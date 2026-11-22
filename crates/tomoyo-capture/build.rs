use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
        compile_linux_cap(&out_dir);
    }
}

fn compile_linux_cap(out_dir: &Path) {
    println!("cargo:rerun-if-changed=src/linux_cap.c");
    println!("cargo:rerun-if-changed=src/linux_portal.c");
    println!("cargo:rerun-if-changed=src/linux_wl_hud.c");
    println!("cargo:rerun-if-changed=src/linux_pw_rec.c");
    let pkgs = [
        "gtk+-3.0", "gdk-3.0", "gdk-wayland-3.0", "wayland-client",
        "cairo", "pangocairo", "gdk-pixbuf-2.0", "gio-2.0", "gio-unix-2.0",
        "x11", "xext", "libpipewire-0.3",
    ];
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
    for (src, name) in [
        ("src/linux_cap.c", "linux_cap.o"),
        ("src/linux_portal.c", "linux_portal.o"),
        ("src/linux_wl_hud.c", "linux_wl_hud.o"),
        ("src/linux_pw_rec.c", "linux_pw_rec.o"),
    ] {
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

    let archive = out_dir.join("libtomoyo_cap_linux.a");
    let _ = std::fs::remove_file(&archive);
    let mut ar_cmd = Command::new("ar");
    ar_cmd.arg("rcs").arg(&archive);
    for o in &objs {
        ar_cmd.arg(o);
    }
    let ar_ok = ar_cmd.status().expect("failed to invoke ar").success();
    assert!(ar_ok, "ar failed");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=tomoyo_cap_linux");

    let mut seen = std::collections::HashSet::new();
    for flag in libs {
        if !seen.insert(flag.clone()) {
            continue;
        }
        if let Some(dir) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
