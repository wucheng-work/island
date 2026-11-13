; Island For Windows installer (NSIS 3). Per-user install, no admin rights needed.
;
; Island.exe is the distributed executable. Existing install locations,
; data, uninstall keys and the Run value remain compatible with older versions.
;
; Build (Linux/macOS/Windows with makensis):
;   makensis -DVERSION=<Cargo version> -DEXE=/abs/path/poni.exe -DOUTDIR=/abs/path/dist installer/poni.nsi
; tools/package.sh does this for you.

; Native 64-bit installer (Poni itself is x64-only).
Target amd64-unicode
SetCompressor /SOLID lzma

!define APPNAME "Island For Windows"
!ifndef VERSION
  !error "Pass -DVERSION=<Cargo workspace version>"
!endif
!define PUBLISHER "Jatevo Labs"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Poni"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!ifndef EXE
  !error "Pass -DEXE=<path to poni.exe>"
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif

Name "${APPNAME}"
OutFile "${OUTDIR}/Island-For-Windows-Setup-${VERSION}.exe"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\Island"
InstallDirRegKey HKCU "Software\Poni" "InstallDir"
BrandingText "${APPNAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "Island For Windows installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (C) 2026 ${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"

; Ask a running Poni to quit (it saves its data on WM_CLOSE), wait up to 3 s,
; then force-close with the fully qualified system taskkill.
!macro StopPoni
  FindWindow $0 "Poni.Island"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
    StrCpy $1 0
    ${DoWhile} $0 <> 0
      ${If} $1 >= 30
        ${Break}
      ${EndIf}
      Sleep 100
      FindWindow $0 "Poni.Island"
      IntOp $1 $1 + 1
    ${Loop}
  ${EndIf}
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM poni.exe /F'
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM Island.exe /F'
  Sleep 200
!macroend
!define MUI_ICON "../crates/poni/res/poni.ico"
!define MUI_UNICON "../crates/poni/res/poni.ico"
; Oyen hanging from the island on the welcome and finish pages, Poni's logo in the header (tools/make_brand.py).
!define MUI_WELCOMEFINISHPAGE_BITMAP "art/nsis-welcome.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "art/nsis-header.bmp"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\Island.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Island For Windows now"
!define MUI_WELCOMEPAGE_TITLE "Welcome to Island For Windows"
!define MUI_WELCOMEPAGE_TEXT "Island For Windows puts a living notch at the top of your screen: music with synced lyrics, a calendar and to-do list, a focus timer, a file shelf and system monitoring.$\r$\n$\r$\nIt installs just for you and doesn't need administrator rights."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Island For Windows" SecMain
  SectionIn RO
  ; Close a running copy so the exe can be replaced.
  !insertmacro StopPoni
  SetOutPath "$INSTDIR"
  Delete "$INSTDIR\poni.exe"
  File "/oname=Island.exe" "${EXE}"
  ; The licenses of what poni.exe is built from (mecab-ipadic asks for its notice in every copy).
  File "../THIRD-PARTY-NOTICES.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  ; Older shortcuts: "Poni" (0.1.0, 0.1.1) and "Oyen Poni" (0.1.2 to the last 0.1.x).
  Delete "$SMPROGRAMS\Poni.lnk"
  Delete "$SMPROGRAMS\Oyen Poni.lnk"
  Delete "$SMPROGRAMS\Oyenza Studio.lnk"
  Delete "$SMPROGRAMS\灵动岛.lnk"
  CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\Island.exe" "" "$INSTDIR\Island.exe" 0
  ; Keep an existing startup registration pointed at the renamed executable.
  ReadRegStr $0 HKCU "${RUN_KEY}" "Poni"
  ${If} $0 != ""
    WriteRegStr HKCU "${RUN_KEY}" "Poni" '"$INSTDIR\Island.exe"'
  ${EndIf}
  WriteRegStr HKCU "Software\Poni" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\Island.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" 7680
SectionEnd

Section "Start Island For Windows with Windows" SecAutostart
  ; Same value the app's own "Start with Windows" toggle writes.
  WriteRegStr HKCU "${RUN_KEY}" "Poni" '"$INSTDIR\Island.exe"'
SectionEnd

; The app's own update runs this Setup silently: `/S /reopen` (crates/poni/src/services/update.rs).
Function .onInit
  ; Silent: Start with Windows stays as the person set it (the Run value already points here).
  ${If} ${Silent}
    !insertmacro UnselectSection ${SecAutostart}
  ${EndIf}
FunctionEnd

Function .onInstSuccess
  ; The finish page is absent in silent mode: /reopen starts the app.
  ${If} ${Silent}
    ${GetParameters} $R0
    ClearErrors
    ${GetOptions} $R0 "/reopen" $R1
    ${IfNot} ${Errors}
      Exec '"$INSTDIR\Island.exe"'
    ${EndIf}
  ${EndIf}
FunctionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "The Island For Windows app and a Start Menu shortcut."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "Open Island For Windows automatically when you sign in. You can change this later from its menu."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro StopPoni
  Delete "$INSTDIR\poni.exe"
  Delete "$INSTDIR\Island.exe"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\Poni.lnk"
  Delete "$SMPROGRAMS\Oyen Poni.lnk"
  Delete "$SMPROGRAMS\Oyenza Studio.lnk"
  Delete "$SMPROGRAMS\灵动岛.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "Poni"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Poni"
  ; Cache and log (lyrics lookups name the songs you played) go too.
  RMDir /r "$LOCALAPPDATA\Poni"
  ; Your settings and data (tasks, shelf, focus history) in %APPDATA%\Poni are kept.
SectionEnd
