# Tomoyo Release Signing, Core/Suite Profiles & SLSA Provenance

Every official Tomoyo GitHub Release ships verifiable integrity proofs so users and package managers can audit binaries without trusting third-party mirrors.

## 1. Core vs Suite Distribution Profiles

Starting with **v3.0.0**, Tomoyo physically separates the minimal launcher microkernel from optional official productivity plugins:

| Profile | Included Binaries & Assets | Use Case |
| --- | --- | --- |
| **Core** (`*-core-portable.zip`, `*-core.tar.gz`, Inno Setup *Minimal Core*, `-Profile Core` / `--profile=core`) | `tomoyo`, `tomoyo-webview-host`, `tomoyo-cli`, bundled Alibaba PuHuiTi 3.0 fonts, sharded `registry/` | Pure ultra-low-memory app/file launcher; install any plugins on demand from Plugin Center |
| **Suite** (default `*-setup.exe`, `*-portable.zip`, `*.tar.gz`) | Everything in Core + `tomoyo-capture` (screenshot, pin, MP4/GIF screen recording) + `tomoyo-plugin-{ocr,notes,md-editor,hello}` + bundled reference plugins | Complete out-of-the-box desktop productivity suite |

## 2. Verifying Release Artifacts (SHA-256 + SLSA Provenance)

1. Verify file checksums against `SHA256SUMS.txt` / `SHA256SUMS-linux.txt`:
   ```powershell
   Get-FileHash Tomoyo-v3.0.0-windows-x64-setup.exe -Algorithm SHA256
   ```
2. Verify GitHub Build Provenance (SLSA Level 3 via `actions/attest-build-provenance`):
   ```bash
   gh attestation verify Tomoyo-v3.0.0-windows-x64-setup.exe --owner q871958549
   gh attestation verify Tomoyo-v3.0.0-linux-x86_64.tar.gz --owner q871958549
   ```

## 3. Optional Windows Authenticode Signing Hook

`.github/workflows/release.yml` automatically signs all Windows release executables and the Inno Setup installer using `signtool.exe` (SHA-256 + RFC 3161 DigiCert timestamp) whenever the following repository secrets are configured:

- `WINDOWS_CERT_PFX_BASE64`: Base64-encoded PKCS#12 (`.pfx`) code-signing certificate (e.g., SignPath Foundation open-source certificate or EV cert).
- `WINDOWS_CERT_PASSWORD`: Passphrase protecting the `.pfx` file.

When those secrets are absent (e.g., community forks or local test tags), the signing step skips cleanly while keeping `SHA256SUMS.txt` and SLSA Build Provenance attestations active.
