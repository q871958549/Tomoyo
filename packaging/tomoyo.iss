; Tomoyo Windows x64 Installer Script (Inno Setup 6)
; Zero-UAC per-user install to %LOCALAPPDATA%\Programs\Tomoyo with clean uninstaller.

#ifndef AppVersion
  #define AppVersion "3.1.0"
#endif

#ifndef StageDir
  #define StageDir "..\dist\stage"
#endif

[Setup]
AppId={{8F4E29A1-6C74-4D3B-9B2A-7E3F50D81C20}
AppName=Tomoyo
AppVersion={#AppVersion}
AppVerName=Tomoyo v{#AppVersion}
AppPublisher=Tomoyo Open Source Contributors
AppPublisherURL=https://github.com/q871958549/Tomoyo
AppSupportURL=https://github.com/q871958549/Tomoyo/issues
AppUpdatesURL=https://github.com/q871958549/Tomoyo/releases
DefaultDirName={localappdata}\Programs\Tomoyo
DefaultGroupName=Tomoyo
AllowNoIcons=yes
LicenseFile=..\LICENSE-MIT
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Tomoyo-v{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\tomoyo.ico
UninstallDisplayIcon={app}\tomoyo.exe
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "suite"; Description: "Full Suite (完整套件：核心启动器 + 截图录屏 + 全部官方插件)"
Name: "core"; Description: "Minimal Core (极简内核：仅主启动器 + 设置中心 + CLI，按需自选插件)"
Name: "custom"; Description: "Custom installation (自定义组件)"; Flags: iscustom

[Components]
Name: "core"; Description: "Tomoyo Core Launcher, Settings Center & CLI (必选内核)"; Types: suite core custom; Flags: fixed
Name: "suite"; Description: "Official Productivity Suite (官方效率套件)"; Types: suite
Name: "suite\capture"; Description: "Native Screenshot, Pin & Screen Recording (原生截图/贴图/录屏)"; Types: suite
Name: "suite\plugins"; Description: "Official & Reference Plugins (剪贴板 / 翻译 / 截图 / 备忘录 / Quiet.md / 离线 OCR)"; Types: suite

[Tasks]
Name: "desktopicon"; Description: "Create a Desktop shortcut (创建桌面快捷方式)"; GroupDescription: "Additional tasks (附加选项):"
Name: "autostart"; Description: "Launch Tomoyo silently at Windows login (开机静默自启驻留托盘)"; GroupDescription: "Additional tasks (附加选项):"; Flags: unchecked

[Files]
Source: "{#StageDir}\tomoyo.exe"; DestDir: "{app}"; Components: core; Flags: ignoreversion
Source: "{#StageDir}\tomoyo-webview-host.exe"; DestDir: "{app}"; Components: core; Flags: ignoreversion
Source: "{#StageDir}\tomoyo-cli.exe"; DestDir: "{app}"; Components: core; Flags: ignoreversion
Source: "{#StageDir}\LICENSE-*"; DestDir: "{app}"; Components: core; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}"; Components: core; Flags: ignoreversion
Source: "{#StageDir}\assets\*"; DestDir: "{app}\assets"; Components: core; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StageDir}\registry\*"; DestDir: "{app}\registry"; Components: core; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StageDir}\tomoyo-capture.exe"; DestDir: "{app}"; Components: suite\capture; Flags: ignoreversion
Source: "{#StageDir}\tomoyo-plugin-*.exe"; DestDir: "{app}"; Components: suite\plugins; Flags: ignoreversion
Source: "{#StageDir}\plugins\*"; DestDir: "{app}\plugins"; Components: suite\plugins; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
Type: files; Name: "{app}\uninstall.ps1"

[Icons]
Name: "{group}\Tomoyo"; Filename: "{app}\tomoyo.exe"; IconFilename: "{app}\tomoyo.exe"
Name: "{group}\Uninstall Tomoyo"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Tomoyo"; Filename: "{app}\tomoyo.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Tomoyo"; ValueData: """{app}\tomoyo.exe"" --silent"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\tomoyo.exe"; Description: "Launch Tomoyo now (立即启动 Tomoyo · Alt+Space 呼出)"; Flags: nowait postinstall skipifsilent

[Code]
function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
begin
  // Gracefully terminate any running Tomoyo processes before replacing binaries
  Exec('taskkill.exe', '/F /IM tomoyo.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-webview-host.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-capture.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-ocr.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-notes.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-md-editor.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-translate.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-capture.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-clipboard.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-plugin-hello.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM tomoyo-cli.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  // Remove legacy scripts/install-local.ps1 uninstall entry so Add/Remove Programs only shows one Tomoyo
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomoyo');
  Result := True;
end;
