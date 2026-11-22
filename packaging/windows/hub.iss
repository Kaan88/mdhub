; Inno Setup script for Hub, the launcher and updater. Per user, like mdedit's: no elevation, every
; registry key under HKCU. Modelled on mdedit.iss. Build: ISCC.exe /DAppVersion=0.1.0 hub.iss
; (tools\build-installer.ps1 builds the release exe first). The release workflow does the same.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define Dash Pos("-", AppVersion)
#if Dash > 0
  #define NumVer Copy(AppVersion, 1, Dash - 1)
#else
  #define NumVer AppVersion
#endif
#define AppName "Hub"
#define AppExe "hub.exe"

[Setup]
; The uninstall key is {A3D5E8C1-7B42-4F96-8E1D-5C0B2F7A9D64}_is1.
AppId={{A3D5E8C1-7B42-4F96-8E1D-5C0B2F7A9D64}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Kaan88
AppPublisherURL=https://github.com/Kaan88/hub-rs
VersionInfoVersion={#NumVer}.0
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
SetupIconFile=..\..\assets\hub.ico
UninstallDisplayIcon={app}\hub.ico
UninstallDisplayName={#AppName}
; Hub with its window closed is a tray process Restart Manager cannot close; [Code] asks it to quit.
CloseApplications=no
RestartApplications=no
OutputDir=..\..\target\installer
OutputBaseFilename=Hub-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start Hub in the tray when I sign in to Windows"; GroupDescription: "Startup:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\..\target\release\hub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\hub.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\hub.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\hub.ico"; Tasks: desktopicon

[Registry]
; The same value Hub's own setting writes (autostart.rs): "<exe>" --tray. Removed on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Hub"; ValueData: """{app}\{#AppExe}"" --tray"; Flags: uninsdeletevalue; Tasks: startup

[Run]
; Skipped by a silent setup (Hub updating Hub swaps its own exe and restarts itself).
Filename: "{app}\{#AppExe}"; Description: "Start Hub"; Flags: nowait postinstall skipifsilent

[Code]
// Ask a running Hub (usually only a tray process) to quit, over its single-instance port, and give
// it a moment, before files are replaced or removed.
procedure QuitRunningHub();
var
  Exe: String;
  Code: Integer;
begin
  Exe := ExpandConstant('{app}\{#AppExe}');
  if FileExists(Exe) then
  begin
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(800);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitRunningHub();
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    QuitRunningHub();
end;
