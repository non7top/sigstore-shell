; Built with `make installer`. Defines: VERSION, VERSION_NUMERIC (x.y.z.w), OUTFILE; PROVENANCE_REPO only for release builds.
!include "x64.nsh"
!include "MUI2.nsh"

!ifndef VERSION
  !error "pass -DVERSION=..."
!endif
!ifndef VERSION_NUMERIC
  !error "pass -DVERSION_NUMERIC=x.y.z.w"
!endif

!define NAME "sigstore-shell"
!define DLL "sigstore_shell_ext.dll"
!define CLSID "{fbcd8210-9f9c-4b07-900a-ad12500a4363}"
!define DESCRIPTION "Sigstore property page"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}"

Name "${NAME} ${VERSION}"
OutFile "${OUTFILE}"
Unicode true
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\${NAME}"
SetCompressor /SOLID lzma

VIProductVersion "${VERSION_NUMERIC}"
VIFileVersion "${VERSION_NUMERIC}"
VIAddVersionKey /LANG=1033 "ProductName" "${NAME}"
VIAddVersionKey /LANG=1033 "FileDescription" "${NAME} installer"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "MIT License"
; Absent on local builds, so they make no claim about where they came from.
!ifdef PROVENANCE_REPO
  VIAddVersionKey /LANG=1033 "ProvenanceRepo" "${PROVENANCE_REPO}"
!endif

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Explorer's own keys and the extension are 64-bit; the installer itself is 32-bit.
!macro Require64
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "${NAME} needs 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
!macroend

Function .onInit
  !insertmacro Require64
FunctionEnd

Function un.onInit
  !insertmacro Require64
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  ; A running Explorer keeps the old DLL mapped; it can be renamed but not overwritten.
  IfFileExists "$INSTDIR\${DLL}" 0 +3
    Delete "$INSTDIR\${DLL}.old"
    Rename "$INSTDIR\${DLL}" "$INSTDIR\${DLL}.old"
  File "../dist/${DLL}"
  Delete /REBOOTOK "$INSTDIR\${DLL}.old"

  ; Same keys as DllRegisterServer (crates/sigstore-shell-ext/src/registration.rs).
  WriteRegStr HKLM "Software\Classes\CLSID\${CLSID}" "" "${DESCRIPTION}"
  WriteRegStr HKLM "Software\Classes\CLSID\${CLSID}\InprocServer32" "" "$INSTDIR\${DLL}"
  WriteRegStr HKLM "Software\Classes\CLSID\${CLSID}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr HKLM "Software\Classes\exefile\shellex\PropertySheetHandlers\SigstoreShell" "" "${CLSID}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved" "${CLSID}" "${DESCRIPTION}"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "non7top"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  DeleteRegKey HKLM "Software\Classes\exefile\shellex\PropertySheetHandlers\SigstoreShell"
  DeleteRegKey HKLM "Software\Classes\CLSID\${CLSID}"
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved" "${CLSID}"
  DeleteRegKey HKLM "${UNINST_KEY}"

  Delete /REBOOTOK "$INSTDIR\${DLL}"
  Delete /REBOOTOK "$INSTDIR\${DLL}.old"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /REBOOTOK "$INSTDIR"

  IfRebootFlag 0 +2
    MessageBox MB_ICONINFORMATION "Explorer still has the extension loaded. The files are removed at the next restart." /SD IDOK
SectionEnd
