use flate2::write::GzEncoder;
use flate2::Compression;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let src = "../../frontend/dist/settings.html";
    println!("cargo:rerun-if-changed={src}");
    let raw = std::fs::read(src).unwrap_or_else(|e| panic!("failed to read {src}: {e}"));
    let mut enc = GzEncoder::new(Vec::with_capacity(raw.len() / 3), Compression::best());
    enc.write_all(&raw).expect("gzip write failed");
    let gz = enc.finish().expect("gzip finish failed");
    std::fs::write(out_dir.join("settings.html.gz"), gz).expect("write compressed html");

    #[cfg(windows)]
    {
        let ico = "../../assets/tomoyo.ico";
        println!("cargo:rerun-if-changed={ico}");
        if Path::new(ico).exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(ico);
            let _ = res.compile();
        }
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        compile_linux_wv(&out_dir);
    }
}

fn compile_linux_wv(out_dir: &Path) {
    println!("cargo:rerun-if-changed=src/linux_wv.c");
    println!("cargo:rerun-if-changed=src/linux_kga.c");
    let pkgs = ["gtk+-3.0", "webkit2gtk-4.1", "gio-2.0", "gdk-3.0"];
    let mut cflags = Vec::new();
    let mut libs = Vec::new();
    for pkg in pkgs {
        let ok = Command::new("pkg-config")
            .args(["--exists", pkg])
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "missing pkg-config package `{pkg}` on Linux");
        if let Ok(out) = Command::new("pkg-config").args(["--cflags", pkg]).output() {
            cflags.extend(
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .map(ToString::to_string),
            );
        }
        if let Ok(out) = Command::new("pkg-config").args(["--libs", pkg]).output() {
            libs.extend(
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .map(ToString::to_string),
            );
        }
    }
    let mut objs = Vec::new();
    for (src, name) in [("src/linux_wv.c", "linux_wv.o"), ("src/linux_kga.c", "linux_kga.o")] {
        let obj = out_dir.join(name);
        let status = Command::new("gcc")
            .args(["-O2", "-fPIC", "-Wall", "-Wextra", "-Werror"])
            .args(&cflags)
            .args(["-c", src, "-o"])
            .arg(&obj)
            .status()
            .expect("failed to invoke gcc");
        assert!(status.success(), "gcc failed compiling {src}");
        objs.push(obj);
    }

    let archive = out_dir.join("libtomoyo_wv_linux.a");
    let _ = std::fs::remove_file(&archive);
    let ar_ok = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .args(&objs)
        .status()
        .expect("failed to invoke ar")
        .success();
    assert!(ar_ok, "ar failed");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=tomoyo_wv_linux");

    let mut seen = std::collections::HashSet::new();
    for tok in libs {
        if let Some(dir) = tok.strip_prefix("-L") {
            if seen.insert(tok.clone()) {
                println!("cargo:rustc-link-search=native={dir}");
            }
        } else if let Some(lib) = tok.strip_prefix("-l")
            && seen.insert(tok.clone())
        {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
