# Tomoyo 分发与软件供应链信任指南 (Distribution & Supply-Chain Trust)

本文档汇总 Tomoyo 的官方分发通道、原生 Windows ARM64 构建、SLSA 构建溯源校验以及开源代码签名接入规范。

---

## 1. SLSA 构建溯源与 SHA-256 校验（已内置）

自 `v2.2.0` 起，每次 GitHub Release 由 GitHub Actions 自动产出：
- **Windows x64**：`Tomoyo-vX.Y.Z-windows-x64-setup.exe`（免 UAC 安装包）、`Tomoyo-vX.Y.Z-windows-x64-portable.zip`、`SHA256SUMS.txt`
- **Windows ARM64**：`Tomoyo-vX.Y.Z-windows-arm64-portable.zip`、`SHA256SUMS-arm64.txt`
- **GitHub Artifact Attestations（SLSA Build Provenance）**：每一份安装包与便携包均附带加密构建证明。

用户可在安装前通过 GitHub CLI 一键核验二进制是否 100% 由公开源码在 GitHub Actions 官方流水线构建：

```powershell
gh attestation verify Tomoyo-v2.2.0-windows-x64-setup.exe -R q871958549/Tomoyo
```

---

## 2. 提交至 Windows Package Manager (`winget-pkgs`)

每个 Release 已自动产出填充好最新 `InstallerSha256` 的 `TomoyoContributors.Tomoyo.installer.yaml`（严格保留 `ManifestVersion: 1.6.0`），配合仓库 `packaging/winget/` 中的 version 与 locale 清单，可使用微软官方 `wingetcreate` 一键提 PR：

```powershell
# 首次提交或版本更新
wingetcreate update TomoyoContributors.Tomoyo `
  --version 2.2.0 `
  --urls "https://github.com/q871958549/Tomoyo/releases/download/v2.2.0/Tomoyo-v2.2.0-windows-x64-setup.exe" `
  --submit
```

---

## 3. 提交至 Scoop（官方 `extras` Bucket）

每个 Release 已自动产出带最新 `hash` 的 `tomoyo.scoop.json`（同时包含 `checkver` 与 `autoupdate` 配置）：

```powershell
# 本地测试清单安装
scoop install https://github.com/q871958549/Tomoyo/releases/download/v2.2.0/tomoyo.scoop.json
```

满足 Scoop `extras` 收录标准后，可将 `packaging/scoop/tomoyo.json` 提交 PR 至 `ScoopInstaller/Extras`。

---

## 4. 开源公益代码签名接入路径（消除 Windows SmartScreen 警告）

Tomoyo 采用 MIT / Apache-2.0 双开源协议，符合 [SignPath Foundation](https://signpath.org/) 免费开源项目 Authenticode 代码签名资格。待申请通过后，只需在 `.github/workflows/release.yml` 的打包步骤与发布步骤之间接入 `signpath/github-action-submit-signing-request` 对 `tomoyo.exe`、`tomoyo-webview-host.exe`、`tomoyo-capture.exe` 及 `setup.exe` 签名，再重新计算 `SHA256SUMS.txt` 即可实现零 SmartScreen 警告安装。
