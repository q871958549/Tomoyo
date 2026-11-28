use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    println!("cargo:rerun-if-changed=src/macos_bridge.m");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
    let obj = out_dir.join("macos_bridge.o");
    let archive = out_dir.join("libtomoyo_ui_macos_objc.a");

    let cc = Command::new("clang")
        .args(["-O2", "-fPIC", "-fobjc-arc", "-Wall", "-Wextra", "-Werror", "-c", "src/macos_bridge.m", "-o"])
        .arg(&obj)
        .status()
        .expect("failed to invoke clang for macOS bridge");
    assert!(cc.success(), "clang failed compiling src/macos_bridge.m");

    let _ = std::fs::remove_file(&archive);
    let ar = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&obj)
        .status()
        .expect("failed to invoke ar for macOS bridge");
    assert!(ar.success(), "ar failed archiving libtomoyo_ui_macos_objc.a");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=tomoyo_ui_macos_objc");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Carbon");
}
