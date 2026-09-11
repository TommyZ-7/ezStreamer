; ezStreamer NSIS installer (Windows, per-user install, no admin required).
;
; Built by .github/workflows/release.yml:
;   cargo build --release -p ezstreamer
;   makensis /DAPP_VERSION=<version> /DSRC_DIR=target\release \
;     /DOUT_FILE=dist\ezStreamer-<version>-setup.exe packaging\windows\ezstreamer.nsi
;
; SRC_DIR must contain:
;   ezstreamer.exe
;   resources\gstreamer\...   (pinned MSVC runtime subset, staged by CI)
;   resources\licenses\...    (GSTREAMER-NOTICE.txt, LGPL provenance)

Unicode true
!include "MUI2.nsh"

!ifndef APP_VERSION
  !define APP_VERSION "0.1.0-preview09"
!endif
!ifndef SRC_DIR
  !define SRC_DIR "..\..\target\release"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "..\..\dist\ezStreamer-${APP_VERSION}-setup.exe"
!endif

!ifndef ICON_FILE
  ; makensis chdirs to the script directory, so this path is relative to
  ; packaging/windows. CI passes an absolute /DICON_FILE.
  !define ICON_FILE "..\..\ezstreamer-app\icons\icon.ico"
!endif

Name "ezStreamer ${APP_VERSION}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\ezStreamer"
InstallDirRegKey HKCU "Software\ezStreamer" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; Per-user shortcuts: shell folders must resolve inside the current-user
; context (the command is only valid in a section/function).
Function .onInit
  SetShellVarContext current
FunctionEnd

!define MUI_ABORTWARNING
!define MUI_ICON "${ICON_FILE}"
!define MUI_UNICON "${ICON_FILE}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\ezstreamer.exe"
!define MUI_FINISHPAGE_RUN_TEXT "ezStreamer を起動する"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Japanese"
!insertmacro MUI_LANGUAGE "English"

Section "ezStreamer" SEC_MAIN
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "${SRC_DIR}\ezstreamer.exe"

  ; Bundled GStreamer runtime + license notices. The app probes both
  ; <exe>\gstreamer and <exe>\resources\gstreamer (core::gst::bundled_*).
  SetOutPath "$INSTDIR\resources"
  File /r "${SRC_DIR}\resources\*.*"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\ezStreamer.lnk" "$INSTDIR\ezstreamer.exe"
  CreateShortcut "$DESKTOP\ezStreamer.lnk" "$INSTDIR\ezstreamer.exe"

  WriteRegStr HKCU "Software\ezStreamer" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "DisplayName" "ezStreamer"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "Publisher" "ezStreamer"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$DESKTOP\ezStreamer.lnk"
  Delete "$SMPROGRAMS\ezStreamer.lnk"
  ; %APPDATA%\ezStreamer (profiles.json / logs) is intentionally kept.
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ezStreamer"
  DeleteRegKey HKCU "Software\ezStreamer"
SectionEnd
