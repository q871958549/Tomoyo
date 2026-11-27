# Local Windows Installation Script for Tomoyo
# Installs built release binaries + reference plugins (native OS UI typography) into
# %LOCALAPPDATA%\Programs\Tomoyo, creates Start Menu & Desktop shortcuts, registers in
# Windows Add/Remove Programs, and optionally launches Tomoyo immediately.

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Tomoyo"),
    [ValidateSet("Suite", "Core")]
    [string]$Profile = "Suite",
    [switch]$NoLaunch,
    [switch]$EnableAutostart,
    [switch]$CoreOnly,
    [switch]$WithExamples
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $repoRoot "target\release"

if (-not (Test-Path (Join-Path $releaseDir "tomoyo.exe"))) {
    throw "Release binary not found at $releaseDir\tomoyo.exe. Run `cargo build --release --workspace` first."
}

# Read version from [workspace.package] in root Cargo.toml
$cargoToml = Get-Content (Join-Path $repoRoot "Cargo.toml") -Raw
$version = if ($cargoToml -match '(?ms)\[workspace\.package\][^\[]*?^\s*version\s*=\s*"([^"]+)"') { $Matches[1] } else { "3.0.0" }

Write-Host "=== Installing Tomoyo v$version ($Profile profile) -> $InstallDir ===" -ForegroundColor Cyan

# Stop any running instances before replacing binaries
Stop-Process -Name "tomoyo","tomoyo-webview-host","tomoyo-capture","tomoyo-plugin-ocr","tomoyo-plugin-notes","tomoyo-plugin-md-editor","tomoyo-plugin-translate","tomoyo-plugin-capture","tomoyo-plugin-clipboard","tomoyo-plugin-hello","tomoyo-cli" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "assets\fonts") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "assets\plugin-icons") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "registry") | Out-Null

# Copy executables & license/docs (Core/-CoreOnly = minimal microkernel; Suite = official toolkit; -WithExamples = dev examples)
$isCore = $CoreOnly -or ($Profile -eq "Core")
$bins = @("tomoyo.exe", "tomoyo-webview-host.exe", "tomoyo-cli.exe")
if (-not $isCore) {
    $bins += @("tomoyo-capture.exe", "tomoyo-plugin-ocr.exe", "tomoyo-plugin-notes.exe", "tomoyo-plugin-md-editor.exe", "tomoyo-plugin-translate.exe", "tomoyo-plugin-capture.exe", "tomoyo-plugin-clipboard.exe")
}
if ($WithExamples) {
    $bins += @("tomoyo-plugin-hello.exe")
}
foreach ($bin in $bins) {
    $srcBin = Join-Path $releaseDir $bin
    if (Test-Path $srcBin) { Copy-Item $srcBin $InstallDir -Force }
}
Copy-Item (Join-Path $repoRoot "LICENSE-MIT"), (Join-Path $repoRoot "LICENSE-APACHE"), (Join-Path $repoRoot "README.md") $InstallDir -Force
Remove-Item (Join-Path $InstallDir "assets\fonts\AlibabaPuHuiTi-*.ttf") -Force -ErrorAction SilentlyContinue
if (Test-Path (Join-Path $repoRoot "assets\fonts\*.ttf")) {
    Copy-Item (Join-Path $repoRoot "assets\fonts\*.ttf") (Join-Path $InstallDir "assets\fonts") -Force
}
Copy-Item (Join-Path $repoRoot "assets\plugin-icons\*.png") (Join-Path $InstallDir "assets\plugin-icons") -Force
Copy-Item (Join-Path $repoRoot "assets\tomoyo.ico") (Join-Path $InstallDir "assets\tomoyo.ico") -Force
Copy-Item (Join-Path $repoRoot "registry\index.json") (Join-Path $InstallDir "registry\index.json") -Force
if (Test-Path (Join-Path $repoRoot "registry\shards")) {
    New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "registry\shards") | Out-Null
    Copy-Item (Join-Path $repoRoot "registry\shards\*.json") (Join-Path $InstallDir "registry\shards") -Force
}
Copy-Item (Join-Path $PSScriptRoot "uninstall-local.ps1") (Join-Path $InstallDir "uninstall.ps1") -Force

# Copy optional bundled plugins (opt-out with -Profile Core / -CoreOnly; developer examples opt-in via -WithExamples)
$pluginsToCopy = @()
if (-not $isCore) {
    $pluginsToCopy += @("builtin-ocr", "official-notes", "official-md-editor", "official-translate", "official-capture", "official-clipboard", "official-devtools-wasm")
}
if ($WithExamples) {
    $pluginsToCopy += @("example-hello", "example-python", "example-wasm", "example-webview-json")
} else {
    foreach ($ex in @("example-hello", "example-python", "example-wasm", "example-webview-json")) {
        Remove-Item (Join-Path $InstallDir "plugins\$ex") -Recurse -Force -ErrorAction SilentlyContinue
    }
}
foreach ($p in $pluginsToCopy) {
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
# Remove any previous Inno Setup uninstall key & stub so Windows never shows duplicate Tomoyo entries
$innoUninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F4E29A1-6C74-4D3B-9B2A-7E3F50D81C20}_is1"
if (Test-Path $innoUninstKey) {
    Remove-Item -Path $innoUninstKey -Recurse -Force -ErrorAction SilentlyContinue
}
Remove-Item -Path (Join-Path $InstallDir "unins000.exe"), (Join-Path $InstallDir "unins000.dat") -Force -ErrorAction SilentlyContinue

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

$runRegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$approvedRunPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\StartupApproved\Run"
$hasLegacyRun = $null -ne (Get-ItemProperty -Path $runRegPath -Name "Tomoyo" -ErrorAction SilentlyContinue)
$hasAutostartTask = $null -ne (Get-ScheduledTask -TaskName "TomoyoAutostart" -ErrorAction SilentlyContinue)
$approvedItem = Get-ItemProperty -Path $approvedRunPath -Name "Tomoyo" -ErrorAction SilentlyContinue
$runDisabledByUser = ($null -ne $approvedItem) -and ($approvedItem.Tomoyo.Length -ge 1) -and (($approvedItem.Tomoyo[0] -band 1) -ne 0)

if ($EnableAutostart -or $hasAutostartTask -or ($hasLegacyRun -and -not $runDisabledByUser)) {
    $autostartMode = "TaskScheduler"
    try {
        $userId = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
        $trigger = New-ScheduledTaskTrigger -AtLogOn -User $userId
        $action = New-ScheduledTaskAction -Execute $exePath -Argument "--silent" -WorkingDirectory $InstallDir
        $principal = New-ScheduledTaskPrincipal -UserId $userId -LogonType Interactive -RunLevel Highest
        $settings = New-ScheduledTaskSettingsSet `
            -AllowStartIfOnBatteries `
            -DontStopIfGoingOnBatteries `
            -StartWhenAvailable `
            -MultipleInstances IgnoreNew `
            -ExecutionTimeLimit ([TimeSpan]::Zero)
        $settings.Priority = 5
        Register-ScheduledTask `
            -TaskName "TomoyoAutostart" `
            -Trigger $trigger `
            -Action $action `
            -Principal $principal `
            -Settings $settings `
            -Description "Tomoyo desktop launcher autostart (--silent)" `
            -Force | Out-Null
        Remove-ItemProperty -Path $runRegPath -Name "Tomoyo" -ErrorAction SilentlyContinue
    } catch {
        $autostartMode = "RegistryRunFallback"
        Set-ItemProperty -Path $runRegPath -Name "Tomoyo" -Value "`"$exePath`" --silent"
    }
    Write-Host "[OK] Autostart configured via $autostartMode." -ForegroundColor Green
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
