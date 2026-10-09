#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

#define MyAppName "JPOP Corpus Tool"
#define MyAppPublisher "Yisoragoto"
#define MyAppExeName "JpopCorpusTool.exe"

[Setup]
AppId={{B5DE0E0F-6737-4FEC-A5DF-06A52E40DB8B}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\JpopCorpusTool
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=JpopCorpusTool-{#MyAppVersion}-windows-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
VersionInfoVersion={#MyAppVersion}
VersionInfoDescription={#MyAppName}
VersionInfoCompany={#MyAppPublisher}
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加选项："; Flags: unchecked

[Files]
Source: "..\dist\JpopCorpusTool\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Dirs]
Name: "{localappdata}\JpopCorpusTool"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\raw\audio"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\raw\lyrics_lrc"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\metadata"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\processed"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\output"; Flags: uninsneveruninstall
Name: "{localappdata}\JpopCorpusTool\backups"; Flags: uninsneveruninstall

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\用户数据目录"; Filename: "{sys}\explorer.exe"; Parameters: """{localappdata}\JpopCorpusTool"""
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "启动 {#MyAppName}"; Flags: nowait postinstall skipifsilent
