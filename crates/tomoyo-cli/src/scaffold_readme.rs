use std::path::Path;

pub(crate) fn write_plugin_readme(dir: &Path, name: &str, slug: &str, lang: &str) -> std::io::Result<()> {
    let build_step = match lang {
        "wasm" => "1. Build WASM: `cargo build --target wasm32-unknown-unknown --release && cp target/wasm32-unknown-unknown/release/*.wasm plugin.wasm`\n2. Dev test:  `tomoyo-cli dev . --build --once`\n",
        "rust" => "1. Build binary: `cargo build --release` (place binary on PATH or set `[runtime].command`)\n2. Verify:       `tomoyo-cli verify .`\n",
        _ => "1. Verify: `tomoyo-cli verify .`\n",
    };
    let doc = format!(
        "# {name}\n\nTomoyo Launcher `{lang}` plugin (`dev.community.{slug}`).\n\n## Quickstart\n\n{build_step}3. Live watch: `tomoyo-cli dev . --query \"sample\"`\n4. Package ZIP: `tomoyo-cli pack .`\n"
    );
    std::fs::write(dir.join("README.md"), doc)
}
