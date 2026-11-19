fn main() {
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
