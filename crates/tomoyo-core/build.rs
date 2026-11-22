use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn pkg_probe(pkgs: &[&str]) -> (Vec<String>, Vec<String>) {
    let mut cflags = Vec::new();
    let mut libs = Vec::new();
    for pkg in pkgs {
        let ok = Command::new("pkg-config")
            .args(["--exists", pkg])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            continue;
        }
        let macro_name = format!("HAVE_{}", pkg.to_ascii_uppercase().replace(['-', '.'], "_"));
        cflags.push(format!("-D{macro_name}=1"));
        if let Ok(out) = Command::new("pkg-config").args(["--cflags", pkg]).output() {
            for tok in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                cflags.push(tok.to_string());
            }
        }
        if let Ok(out) = Command::new("pkg-config").args(["--libs", pkg]).output() {
            for tok in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                libs.push(tok.to_string());
            }
        }
    }
    (cflags, libs)
}

fn compile_c_lib(out_dir: &Path, name: &str, srcs: &[&str], cflags: &[String]) {
    let mut objs = Vec::new();
    for src in srcs {
        println!("cargo:rerun-if-changed={src}");
        let stem = src.replace(['/', '.'], "_");
        let obj = out_dir.join(format!("{stem}.o"));
        let status = Command::new("gcc")
            .arg("-O2")
            .arg("-fPIC")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .args(cflags)
            .arg("-c")
            .arg(src)
            .arg("-o")
            .arg(&obj)
            .status()
            .expect("failed to invoke gcc for Linux native shim");
        assert!(status.success(), "gcc failed compiling {src}");
        objs.push(obj);
    }
    let archive = out_dir.join(format!("lib{name}.a"));
    let _ = std::fs::remove_file(&archive);
    let status = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .args(&objs)
        .status()
        .expect("failed to invoke ar");
    assert!(status.success(), "ar failed archiving {name}");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static={name}");
}

fn emit_pkg_libs(libs: &[String]) {
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let (cflags, libs) = pkg_probe(&[
        "libcurl",
        "fontconfig",
        "gdk-pixbuf-2.0",
        "libsecret-1",
        "gio-2.0",
    ]);
    compile_c_lib(
        &out_dir,
        "tomoyo_core_linux",
        &["src/linux_sys.c", "src/linux_dbus.c"],
        &cflags,
    );
    emit_pkg_libs(&libs);
}
