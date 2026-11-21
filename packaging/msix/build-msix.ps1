# Builds a staged Windows MSIX package from target\release using Windows SDK MakeAppx.exe
param(
    [string]$Version = "2.2.2",
    [string]$StageDir = "dist\msix-stage",
    [string]$OutPath = "dist\Tomoyo-v2.2.2-x64.msix"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$rel = Join-Path $repoRoot "target\release"

if (-not (Test-Path (Join-Path $rel "tomoyo.exe"))) {
    throw "Release binary not found. Run `cargo build --release --workspace` first."
}

$stageAbs = Join-Path $repoRoot $StageDir
New-Item -ItemType Directory -Force -Path $stageAbs, (Join-Path $stageAbs "assets\fonts") | Out-Null

foreach ($bin in @("tomoyo.exe", "tomoyo-webview-host.exe", "tomoyo-capture.exe", "tomoyo-cli.exe", "tomoyo-plugin-hello.exe", "tomoyo-plugin-ocr.exe", "tomoyo-plugin-notes.exe")) {
    if (Test-Path (Join-Path $rel $bin)) {
        Copy-Item (Join-Path $rel $bin) $stageAbs -Force
    }
}
Copy-Item (Join-Path $repoRoot "assets\tomoyo.ico"), (Join-Path $repoRoot "assets\tomoyo-icon.png") (Join-Path $stageAbs "assets") -Force
Copy-Item (Join-Path $repoRoot "assets\fonts\*.ttf") (Join-Path $stageAbs "assets\fonts") -Force

$manifestRaw = Get-Content (Join-Path $PSScriptRoot "AppxManifest.xml") -Raw
$manifestRaw = [regex]::Replace($manifestRaw, 'Version="\d+\.\d+\.\d+\.\d+"', "Version=`"$Version.0`"")
Set-Content -Path (Join-Path $stageAbs "AppxManifest.xml") -Value $manifestRaw -Encoding UTF8

$makeAppx = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeAppx) {
    Write-Warning "Windows SDK makeappx.exe not installed on this host; MSIX staging prepared cleanly at $stageAbs."
    exit 0
}
& $makeAppx.FullName pack /d $stageAbs /p (Join-Path $repoRoot $OutPath) /o
