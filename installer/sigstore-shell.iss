; Inno Setup script. Not built in this repository's containers (Inno Setup needs Windows or Wine).
[Setup]
AppName=sigstore-shell
AppVersion=0.1.0
DefaultDirName={autopf}\sigstore-shell
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
OutputBaseFilename=sigstore-shell-setup
Compression=lzma2
; Sign the installer too once a certificate is available (SignTool=...).

[Files]
Source: "..\dist\sigstore_shell_ext.dll"; DestDir: "{app}"; Flags: restartreplace uninsrestartdelete regserver
