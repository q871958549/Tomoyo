//! Single-instance flock, `$XDG_RUNTIME_DIR/tomoyo-ipc.sock` listener & async-signal-safe `SIGUSR1` relay.

use std::ffi::c_void;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

const SIGUSR1: i32 = 10;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

static SIG_PIPE_WR: AtomicI32 = AtomicI32::new(-1);

#[repr(C)]
struct SigAction {
    sa_handler: usize,
    sa_mask: [u64; 16],
    sa_flags: i32,
    sa_restorer: usize,
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn pipe(fds: *mut i32) -> i32;
    fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn sigaction(signum: i32, act: *const SigAction, oldact: *mut SigAction) -> i32;
    fn tomoyo_linux_schedule_idle(cb: unsafe extern "C" fn(*mut c_void), data: *mut c_void);
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn ipc_socket_path() -> PathBuf {
    runtime_dir().join("tomoyo-ipc.sock")
}

pub fn pid_file_path() -> PathBuf {
    runtime_dir().join("tomoyo.pid")
}

pub struct SingletonGuard {
    _lock_file: File,
    pid_path: PathBuf,
    sock_path: PathBuf,
}

impl SingletonGuard {
    pub fn acquire() -> Option<Self> {
        let dir = runtime_dir();
        let lock_path = dir.join("tomoyo.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .ok()?;
        let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if rc != 0 {
            return None;
        }
        let pid_path = pid_file_path();
        let _ = std::fs::write(&pid_path, std::process::id().to_string());
        Some(Self {
            _lock_file: file,
            pid_path,
            sock_path: ipc_socket_path(),
        })
    }
}

impl Drop for SingletonGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pid_path);
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

pub fn wake_existing_instance(silent: bool) {
    if silent {
        return;
    }
    if let Ok(mut stream) = UnixStream::connect(ipc_socket_path()) {
        let _ = stream.write_all(b"show\n");
        tracing::info!("Signaled existing Linux Tomoyo instance to show launcher.");
    }
}

extern "C" fn sigusr1_handler(_sig: i32) {
    let fd = SIG_PIPE_WR.load(Ordering::Relaxed);
    if fd >= 0 {
        let b = b'R';
        unsafe { let _ = write(fd, (&b as *const u8).cast::<c_void>(), 1); }
    }
}

pub fn schedule_on_ui<F: FnOnce() + Send + 'static>(f: F) {
    let boxed: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(f));
    unsafe extern "C" fn trampoline(ud: *mut c_void) {
        let task = unsafe { Box::from_raw(ud as *mut Box<dyn FnOnce() + Send>) };
        task();
    }
    unsafe { tomoyo_linux_schedule_idle(trampoline, Box::into_raw(boxed).cast()); }
}

pub fn start_ipc_listeners<F>(alive: Arc<AtomicBool>, on_cmd: F)
where
    F: Fn(String) + Send + Sync + 'static,
{
    let cb = Arc::new(on_cmd);
    let sock = ipc_socket_path();
    let _ = std::fs::remove_file(&sock);
    if let Ok(listener) = UnixListener::bind(&sock) {
        let a1 = Arc::clone(&alive);
        let c1 = Arc::clone(&cb);
        let _ = std::thread::Builder::new().name("tomoyo-ipc-sock".into()).spawn(move || {
            for stream in listener.incoming() {
                if !a1.load(Ordering::Acquire) {
                    break;
                }
                if let Ok(mut s) = stream {
                    let mut buf = String::new();
                    let mut reader = BufReader::new(&mut s);
                    while reader.read_line(&mut buf).unwrap_or(0) > 0 {
                        let line = buf.trim().to_string();
                        buf.clear();
                        if !line.is_empty() {
                            let c_inner = Arc::clone(&c1);
                            schedule_on_ui(move || c_inner(line));
                        }
                    }
                }
            }
        });
    }

    let mut fds = [-1i32; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } == 0 {
        SIG_PIPE_WR.store(fds[1], Ordering::Release);
        let act = SigAction {
            sa_handler: sigusr1_handler as *const () as usize,
            sa_mask: [0; 16],
            sa_flags: 0x1000_0000, // SA_RESTART
            sa_restorer: 0,
        };
        unsafe { sigaction(SIGUSR1, &act, std::ptr::null_mut()); }
        let rd_fd = fds[0];
        let a2 = Arc::clone(&alive);
        let c2 = Arc::clone(&cb);
        let _ = std::thread::Builder::new().name("tomoyo-sigusr1".into()).spawn(move || {
            let mut b = [0u8; 16];
            while a2.load(Ordering::Acquire) {
                let n = unsafe { read(rd_fd, b.as_mut_ptr().cast::<c_void>(), b.len()) };
                if n <= 0 {
                    break;
                }
                let c_inner = Arc::clone(&c2);
                schedule_on_ui(move || c_inner("reload:1".into()));
            }
            unsafe { close(rd_fd); }
        });
    }
}

#[allow(dead_code)]
fn _touch_read_trait<R: Read>(_r: R) {}
