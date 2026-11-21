use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
    for name in ["settings.html", "translator.html", "clipboard.html", "ocr.html"] {
        let src = format!("../../frontend/dist/{name}");
        println!("cargo:rerun-if-changed={src}");
        let raw = std::fs::read(&src).unwrap_or_else(|e| panic!("failed to read {src}: {e}"));
        let mut enc = GzEncoder::new(Vec::with_capacity(raw.len() / 3), Compression::best());
        enc.write_all(&raw).expect("gzip write failed");
        let gz = enc.finish().expect("gzip finish failed");
        std::fs::write(out_dir.join(format!("{name}.gz")), gz).expect("write compressed html");
    }

    #[cfg(windows)]
    {
        let ico = "../../assets/tomoyo.ico";
        println!("cargo:rerun-if-changed={ico}");
        if std::path::Path::new(ico).exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(ico);
            let _ = res.compile();
        }
    }
}
