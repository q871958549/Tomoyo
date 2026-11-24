# Clean Uninstaller for locally installed Tomoyo

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Tomoyo")
)

Stop-Process -Name "tomoyo","tomoyo-webview-host","tomoyo-capture","tomoyo-plugin-ocr","tomoyo-plugin-notes","tomoyo-plugin-md-editor","tomoyo-plugin-hello","tomoyo-cli" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

Remove-Item -Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Tomoyo") -Recurse -Force -ErrorAction SilentlyContinue
$deskLnk = Join-Path ([Environment]::GetFolderPath("Desktop")) "Tomoyo.lnk"
Remove-Item -Path $deskLnk -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomoyo" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F4E29A1-6C74-4D3B-9B2A-7E3F50D81C20}_is1" -Recurse -Force -ErrorAction SilentlyContinue
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "Tomoyo" -ErrorAction SilentlyContinue

if (Test-Path $InstallDir) {
    Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "[OK] Tomoyo uninstalled cleanly." -ForegroundColor Green
