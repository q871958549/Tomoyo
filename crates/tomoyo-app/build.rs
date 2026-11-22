use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
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

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
        compile_linux_shell(&out_dir);
    }
}

fn compile_linux_shell(out_dir: &Path) {
    println!("cargo:rerun-if-changed=src/linux/shell.c");
    println!("cargo:rerun-if-changed=src/linux/pango_draw.c");
    println!("cargo:rerun-if-changed=src/linux/kga_x11.c");
    println!("cargo:rerun-if-changed=src/linux/wl_clip.c");
    let pkgs = [
        "gtk+-3.0",
        "gdk-3.0",
        "cairo",
        "pangocairo",
        "gio-2.0",
        "gio-unix-2.0",
        "ayatana-appindicator3-0.1",
        "wayland-client",
        "x11",
        "xtst",
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
        ("src/linux/shell.c", "linux_app_shell.o"),
        ("src/linux/pango_draw.c", "linux_app_pango.o"),
        ("src/linux/kga_x11.c", "linux_app_kga.o"),
        ("src/linux/wl_clip.c", "linux_app_wlclip.o"),
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

    let archive = out_dir.join("libtomoyo_app_linux.a");
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
    println!("cargo:rustc-link-lib=static=tomoyo_app_linux");

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
