; Tomoyo Windows x64 Installer Script (Inno Setup 6)
; Zero-UAC per-user install to %LOCALAPPDATA%\Programs\Tomoyo with clean uninstaller.

#ifndef AppVersion
  #define AppVersion "1.4.1"
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

[Tasks]
Name: "desktopicon"; Description: "Create a Desktop shortcut (创建桌面快捷方式)"; GroupDescription: "Additional tasks (附加选项):"
Name: "autostart"; Description: "Launch Tomoyo silently at Windows login (开机静默自启驻留托盘)"; GroupDescription: "Additional tasks (附加选项):"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

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
  Result := True;
end;
