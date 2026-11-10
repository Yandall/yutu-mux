; YutuMux Windows installer (Inno Setup 6).
;
; Built in CI from the staged folder produced by
; packaging/windows/build-windows.ps1: every runtime dependency (mpv +
; ffmpeg DLLs, yt-dlp.exe) is already beside yutu-mux.exe, so the
; installer copies files only and never downloads or installs anything
; else. Per-user install, no admin rights needed.
;
;   ISCC.exe /DAppVersion="0.1.0" /DStageDir="dist\yutu-mux-0.1.0-windows-x86_64\yutu-mux-0.1.0-windows-x86_64" ^
;     /O"dist\output" /F"YutuMux-0.1.0-windows-x86_64-Setup" packaging\windows\YutuMux.iss
;
; AppId is a stable random GUID: keep it unchanged across releases so
; upgrades replace the previous install instead of piling up entries in
; Add/Remove Programs.

#ifndef AppVersion
#define AppVersion "0.1.0"
#endif
#ifndef StageDir
#define StageDir "."
#endif

#define MyAppName "YutuMux"
#define MyAppExe "yutu-mux.exe"

[Setup]
AppId={{BA502948-9B7F-45D6-83F1-511FA55F6F38}}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher=YutuMux contributors
AppPublisherURL=https://github.com/Yandall/yutu-mux
AppSupportURL=https://github.com/Yandall/yutu-mux/issues
DefaultDirName={localappdata}\Programs\YutuMux
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
UninstallDisplayName=YutuMux
SetupIconFile=yutu-mux.ico
SolidCompression=yes
WizardStyle=modern
OutputBaseFilename=YutuMux-Setup
Compression=lzma2/max

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\YutuMux"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\yutu-mux.ico"; WorkingDir: "{app}"
Name: "{autodesktop}\YutuMux"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\yutu-mux.ico"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch YutuMux"; Flags: nowait postinstall skipifsilent
