# Local Windows Installation Script for Tomoyo
# Installs built release binaries + Alibaba PuHuiTi 3.0 fonts + reference plugins into
# %LOCALAPPDATA%\Programs\Tomoyo, creates Start Menu & Desktop shortcuts, registers in
# Windows Add/Remove Programs, and optionally launches Tomoyo immediately.

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Tomoyo"),
    [switch]$NoLaunch,
    [switch]$EnableAutostart
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $repoRoot "target\release"

if (-not (Test-Path (Join-Path $releaseDir "tomoyo.exe"))) {
    throw "Release binary not found at $releaseDir\tomoyo.exe. Run `cargo build --release --workspace` first."
}

# Read version from root Cargo.toml
$cargoToml = Get-Content (Join-Path $repoRoot "Cargo.toml") -Raw
$version = if ($cargoToml -match 'version\s*=\s*"([^"]+)"') { $Matches[1] } else { "0.5.0" }

Write-Host "=== Installing Tomoyo v$version -> $InstallDir ===" -ForegroundColor Cyan

# Stop any running instances before replacing binaries
Stop-Process -Name "tomoyo","tomoyo-webview-host" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "assets\fonts") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "registry") | Out-Null

# Copy executables & license/docs
foreach ($bin in @("tomoyo.exe", "tomoyo-webview-host.exe", "tomoyo-cli.exe", "tomoyo-plugin-hello.exe")) {
    Copy-Item (Join-Path $releaseDir $bin) $InstallDir -Force
}
Copy-Item (Join-Path $repoRoot "LICENSE-MIT"), (Join-Path $repoRoot "LICENSE-APACHE"), (Join-Path $repoRoot "README.md") $InstallDir -Force
Copy-Item (Join-Path $repoRoot "assets\fonts\*.ttf") (Join-Path $InstallDir "assets\fonts") -Force
Copy-Item (Join-Path $repoRoot "assets\tomoyo.ico") (Join-Path $InstallDir "assets\tomoyo.ico") -Force
Copy-Item (Join-Path $repoRoot "registry\index.json") (Join-Path $InstallDir "registry\index.json") -Force
Copy-Item (Join-Path $PSScriptRoot "uninstall-local.ps1") (Join-Path $InstallDir "uninstall.ps1") -Force

# Copy bundled reference plugins (excluding build caches)
foreach ($p in @("example-hello", "example-python", "example-wasm", "example-webview-json")) {
    $dstP = Join-Path $InstallDir "plugins\$p"
    New-Item -ItemType Directory -Force -Path $dstP | Out-Null
    Get-ChildItem (Join-Path $repoRoot "plugins\$p") -Exclude "target","__pycache__" |
        Copy-Item -Destination $dstP -Recurse -Force
}

# Create Start Menu & Desktop shortcuts
$wsh = New-Object -ComObject WScript.Shell
$exePath = Join-Path $InstallDir "tomoyo.exe"
$startMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Tomoyo"
New-Item -ItemType Directory -Force -Path $startMenuDir | Out-Null

$lnkStart = $wsh.CreateShortcut((Join-Path $startMenuDir "Tomoyo.lnk"))
$lnkStart.TargetPath = $exePath
$lnkStart.WorkingDirectory = $InstallDir
$lnkStart.IconLocation = "$exePath,0"
$lnkStart.Description = "Tomoyo — 100% Open-Source Ultra-Low-Memory Rust Launcher"
$lnkStart.Save()

$desktopDir = [Environment]::GetFolderPath("Desktop")
if (Test-Path $desktopDir) {
    $lnkDesk = $wsh.CreateShortcut((Join-Path $desktopDir "Tomoyo.lnk"))
    $lnkDesk.TargetPath = $exePath
    $lnkDesk.WorkingDirectory = $InstallDir
    $lnkDesk.IconLocation = "$exePath,0"
    $lnkDesk.Description = "Tomoyo — Open Rust Launcher"
    $lnkDesk.Save()
}

# Register in Windows Add/Remove Programs (HKCU — zero UAC required)
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomoyo"
New-Item -Path $uninstKey -Force | Out-Null
$installSizeKb = [math]::Round(((Get-ChildItem $InstallDir -Recurse -File | Measure-Object Length -Sum).Sum / 1KB))
Set-ItemProperty -Path $uninstKey -Name "DisplayName" -Value "Tomoyo"
Set-ItemProperty -Path $uninstKey -Name "DisplayVersion" -Value $version
Set-ItemProperty -Path $uninstKey -Name "Publisher" -Value "Tomoyo Open Source Contributors"
Set-ItemProperty -Path $uninstKey -Name "InstallLocation" -Value $InstallDir
Set-ItemProperty -Path $uninstKey -Name "DisplayIcon" -Value "$exePath,0"
Set-ItemProperty -Path $uninstKey -Name "URLInfoAbout" -Value "https://github.com/q871958549/Tomoyo"
Set-ItemProperty -Path $uninstKey -Name "UninstallString" -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $InstallDir 'uninstall.ps1')`""
Set-ItemProperty -Path $uninstKey -Name "EstimatedSize" -Value $installSizeKb -Type DWord
Set-ItemProperty -Path $uninstKey -Name "NoModify" -Value 1 -Type DWord
Set-ItemProperty -Path $uninstKey -Name "NoRepair" -Value 1 -Type DWord

if ($EnableAutostart) {
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "Tomoyo" -Value "`"$exePath`" --silent"
}

Write-Host "[OK] Installed Tomoyo v$version ($installSizeKb KB) to $InstallDir" -ForegroundColor Green
Write-Host "[OK] Start Menu & Desktop shortcuts created; registered in Windows Installed Apps." -ForegroundColor Green

if (-not $NoLaunch) {
    $launchedDetached = $false
    try {
        $taskName = "TomoyoDetachedLaunch"
        $action = New-ScheduledTaskAction -Execute $exePath -Argument "--silent" -WorkingDirectory $InstallDir
        $principal = New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive
        Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
        Start-ScheduledTask -TaskName $taskName
        Start-Sleep -Milliseconds 900
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
        $launchedDetached = $true
    } catch {
        Unregister-ScheduledTask -TaskName "TomoyoDetachedLaunch" -Confirm:$false -ErrorAction SilentlyContinue
        Start-Process -FilePath $exePath -WorkingDirectory $InstallDir -ArgumentList "--silent"
    }
    $running = Get-Process -Name "tomoyo" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($running) {
        $modeNote = if ($launchedDetached) { "detached Session $($running.SessionId)" } else { "direct Session $($running.SessionId)" }
        Write-Host "[OK] Tomoyo launched in tray (PID=$($running.Id), $modeNote)! Press Alt+Space (or your custom hotkey) to summon." -ForegroundColor Green
    } else {
        Write-Warning "Tomoyo did not stay running after launch; check $env:LOCALAPPDATA\Tomoyo\logs\tomoyo.log"
    }
}
