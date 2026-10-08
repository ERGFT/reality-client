; SPDX-License-Identifier: GPL-3.0-or-later
; Installs the complete Windows client from one downloadable executable.

#define AppName "Reality Client"
#define AppVersion "0.1.0"
#define PackageDir "rust-client\dist\windows-x64"

[Setup]
AppId={{F29F8A95-45B6-48E5-AC84-392B6D192874}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=ERGFT
DefaultDirName={localappdata}\Programs\Reality Client
DefaultGroupName=Reality Client
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
UninstallDisplayIcon={app}\RealityClient-Rust.exe
OutputDir=rust-client\dist
OutputBaseFilename=RealityClient-Setup-windows-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; GroupDescription: "Дополнительные ярлыки:"; Flags: unchecked

[Files]
Source: "{#PackageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Reality Client"; Filename: "{app}\RealityClient-Rust.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Reality Client"; Filename: "{app}\RealityClient-Rust.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\RealityClient-Rust.exe"; Description: "Запустить Reality Client"; WorkingDir: "{app}"; Flags: postinstall nowait skipifsilent
