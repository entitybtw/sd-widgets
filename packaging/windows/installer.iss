; Optional graphical installer for the sd-widgets daemon on Windows.
;
; The release ships a portable zip by default; this script is for people who
; would rather install from a wizard. Build it on a Windows machine with
; Inno Setup 6 after build.ps1 has staged the files:
;
;   pwsh -File packaging\windows\build.ps1
;   iscc /DAppVersion=0.6.13 packaging\windows\installer.iss
;
; It reads ..\..\target\windows-stage\sdw-daemon (produced by build.ps1) and
; writes dist\sdw-daemon-<version>-windows-x86_64-setup.exe.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "sd-widgets daemon"
#define AppExe "sdw-daemon.exe"
#define StageDir "..\..\target\windows-stage\sdw-daemon"
#define DistDir "..\..\dist"

[Setup]
AppId={{6B2C7E0A-4F1B-4C7E-9D2A-1E5C0B9A7D31}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=entitybtw
AppPublisherURL=https://git.entitybtw.ru/entitybtw/sd-widgets
DefaultDirName={autopf}\sd-widgets
DefaultGroupName=sd-widgets
DisableProgramGroupPage=yes
OutputDir={#DistDir}
OutputBaseFilename=sdw-daemon-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; The daemon runs fine without Administrator; driver installs ask for it later.
PrivilegesRequired=lowest
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"
Name: "autostart"; Description: "Start sd-widgets daemon when Windows starts"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "sdw-daemon"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
