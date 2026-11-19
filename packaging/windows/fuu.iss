; Inno Setup script for the Fuu installer (Fuu-<version>-windows-x64-setup.exe).
; Built by scripts/package-windows.ps1, which passes the version and paths:
;   ISCC /DAppVersion=0.1.0 /DSourceDir=<staged files> /DOutputDir=<dist> fuu.iss
;
; Installs per user by default (no administrator rights; %LOCALAPPDATA%\Programs\Fuu);
; the user may choose an all-users install instead. Uninstalling removes the
; program only: accounts, the mail cache and keychain entries stay, so a
; reinstall picks them up again.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef SourceDir
  #error Pass /DSourceDir=<folder with Fuu.exe, LICENSE, README.md>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Never change AppId: upgrades and uninstall entries are matched by it.
AppId={{F659A833-A853-41D2-B427-E65CDFCC0B52}
AppName=Fuu
AppVerName=Fuu {#AppVersion}
AppVersion={#AppVersion}
AppCopyright=GPL-3.0-only
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Fuu mail client installer
DefaultDirName={autopf}\Fuu
DefaultGroupName=Fuu
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\Fuu.exe
UninstallDisplayName=Fuu
; Close a running Fuu before replacing it.
CloseApplications=yes
RestartApplications=no
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=Fuu-{#AppVersion}-windows-x64-setup

[Languages]
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\Fuu.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Fuu"; Filename: "{app}\Fuu.exe"
Name: "{autodesktop}\Fuu"; Filename: "{app}\Fuu.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Fuu.exe"; Description: "{cm:LaunchProgram,Fuu}"; Flags: nowait postinstall skipifsilent
