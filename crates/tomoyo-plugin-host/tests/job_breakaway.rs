#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use tomoyo_plugin_host::JobGuard;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

fn run_child_roles_if_requested() -> bool {
    let Ok(role) = std::env::var("TOMOYO_JOB_BREAKAWAY_ROLE") else {
        return false;
    };
    let marker = PathBuf::from(std::env::var("TOMOYO_JOB_BREAKAWAY_MARKER").unwrap());
    match role.as_str() {
        "middle" => {
            // Wait slightly so the parent test has assigned `middle` to JobGuard::assign_webview_child.
            std::thread::sleep(std::time::Duration::from_millis(120));
            let self_exe = std::env::current_exe().unwrap();
            let res = Command::new(self_exe)
                .env("TOMOYO_JOB_BREAKAWAY_ROLE", "grandchild")
                .env("TOMOYO_JOB_BREAKAWAY_MARKER", &marker)
                .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
                .spawn();
            match res {
                Ok(child) => {
                    let _ = std::fs::write(&marker, format!("PID:{}", child.id()));
                    std::thread::sleep(std::time::Duration::from_secs(20));
                }
                Err(e) => {
                    let _ = std::fs::write(&marker, format!("ERR:{}", e.raw_os_error().unwrap_or(-1)));
                    std::thread::sleep(std::time::Duration::from_secs(20));
                }
            }
        }
        "grandchild" => {
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
        _ => {}
    }
    std::process::exit(0);
}

#[test]
fn webview_job_guard_kills_child_on_drop_and_allows_child_breakaway() {
    if run_child_roles_if_requested() {
        return;
    }

    let marker = std::env::temp_dir().join(format!(
        "tomoyo_job_breakaway_{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);

    let self_exe = std::env::current_exe().expect("current_exe");
    let mut middle = Command::new(&self_exe)
        .args(["--exact", "webview_job_guard_kills_child_on_drop_and_allows_child_breakaway"])
        .env("TOMOYO_JOB_BREAKAWAY_ROLE", "middle")
        .env("TOMOYO_JOB_BREAKAWAY_MARKER", &marker)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("spawn middle role");

    let guard = JobGuard::assign_webview_child(&middle, 256)
        .expect("assign_webview_child should succeed");

    // Wait up to 2.5s for `middle` to spawn `grandchild` and record the marker.
    let mut content = String::new();
    for _ in 0..50 {
        if let Ok(s) = std::fs::read_to_string(&marker)
            && !s.is_empty()
        {
            content = s;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&marker);

    drop(guard);
    std::thread::sleep(std::time::Duration::from_millis(200));

    let middle_status = middle.try_wait().expect("try_wait middle");
    assert!(
        middle_status.is_some(),
        "middle process inside JobGuard must terminate when JobGuard drops"
    );

    if let Some(pid_str) = content.strip_prefix("PID:") {
        let pid: u32 = pid_str.trim().parse().expect("valid grandchild pid");
        let check = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("tasklist check");
        let out = String::from_utf8_lossy(&check.stdout);
        assert!(
            out.contains(&pid.to_string()),
            "grandchild pid {pid} spawned with CREATE_BREAKAWAY_FROM_JOB must survive JobGuard close; tasklist={out}"
        );
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    } else if content.trim() != "ERR:5" {
        panic!("unexpected marker state from middle role: {content:?}");
    }
}
