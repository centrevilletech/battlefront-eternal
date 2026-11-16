; Battlefront Eternal native Windows per-user NSIS installer.
; Defines the per-user install layout, directory picker, shortcuts, and
; uninstaller cleanup rules.

Unicode true
SetCompressor /SOLID lzma

!ifndef PRODUCT_NAME
  !define PRODUCT_NAME "Battlefront Eternal"
!endif
!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.1.0"
!endif
!ifndef SOURCE_DIR
  !error "SOURCE_DIR must be defined"
!endif
!ifndef OUT_FILE
  !error "OUT_FILE must be defined"
!endif

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define UNINSTALLER_NAME "Uninstall ${PRODUCT_NAME}.exe"

Name "${PRODUCT_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
CRCCheck on

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "StrFunc.nsh"
${StrStr}
${UnStrStr}

!ifdef ICON_FILE
  !define MUI_ICON "${ICON_FILE}"
  !define MUI_UNICON "${ICON_FILE}"
!endif

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install" SecInstall
  SetShellVarContext current
  ; Preview 331 and older (the Electron builds) installed into this same folder, with an
  ; uninstaller of the same name and an uninstall entry of their own. Their uninstaller did not
  ; remove them when run from here on Windows, so the files are removed by name instead (the
  ; Electron layout of preview 331), and the entry by what it points at. The shortcuts have the
  ; same names as ours and are replaced below. The player's settings and game data in %APPDATA%
  ; are never touched.
  IfFileExists "$INSTDIR\Battlefront Eternal.exe" 0 no_electron
    Delete "$INSTDIR\Battlefront Eternal.exe"
    Delete "$INSTDIR\chrome_100_percent.pak"
    Delete "$INSTDIR\chrome_200_percent.pak"
    Delete "$INSTDIR\d3dcompiler_47.dll"
    Delete "$INSTDIR\dxcompiler.dll"
    Delete "$INSTDIR\dxil.dll"
    Delete "$INSTDIR\ffmpeg.dll"
    Delete "$INSTDIR\icudtl.dat"
    Delete "$INSTDIR\LICENSE.electron.txt"
    Delete "$INSTDIR\LICENSES.chromium.html"
    Delete "$INSTDIR\resources.pak"
    Delete "$INSTDIR\snapshot_blob.bin"
    Delete "$INSTDIR\v8_context_snapshot.bin"
    Delete "$INSTDIR\vk_swiftshader_icd.json"
    Delete "$INSTDIR\vk_swiftshader.dll"
    Delete "$INSTDIR\vulkan-1.dll"
    Delete "$INSTDIR\libEGL.dll"
    Delete "$INSTDIR\libGLESv2.dll"
    Delete "$INSTDIR\${UNINSTALLER_NAME}"
    RMDir /r "$INSTDIR\locales"
    ; app.asar, the Node.js server and the old tools; ours are written right after.
    RMDir /r "$INSTDIR\resources"
    ; Its uninstall entry: one named like ours, under another key, whose uninstaller is here.
    StrCpy $0 0
    electron_entries:
      EnumRegKey $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall" $0
      StrCmp $1 "" no_electron
      StrCmp $1 "${PRODUCT_NAME}" electron_next
      ReadRegStr $2 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
      StrCmp $2 "${PRODUCT_NAME}" 0 electron_next
      ReadRegStr $3 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
      ${StrStr} $4 $3 "$INSTDIR\"
      StrCmp $4 "" electron_next
      DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$1"
      Goto electron_entries
    electron_next:
      IntOp $0 $0 + 1
      Goto electron_entries
  no_electron:
  SetOutPath "$INSTDIR"
  File /r "${SOURCE_DIR}\*.*"

  WriteUninstaller "$INSTDIR\${UNINSTALLER_NAME}"

  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}.lnk" "$INSTDIR\bfe-client.exe" "" "$INSTDIR\bfe-client.exe" 0
  CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\bfe-client.exe" "" "$INSTDIR\bfe-client.exe" 0

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Battlefront Eternal Contributors"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\bfe-client.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" "$\"$INSTDIR\${UNINSTALLER_NAME}$\""
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" "$\"$INSTDIR\${UNINSTALLER_NAME}$\" /S"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  SetShellVarContext current

  Delete "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
  Delete "$DESKTOP\${PRODUCT_NAME}.lnk"

  RMDir /r "$INSTDIR\resources"
  Delete "$INSTDIR\bfe-client.exe"
  Delete "$INSTDIR\bfeserver.exe"
  Delete "$INSTDIR\${UNINSTALLER_NAME}"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "${UNINSTALL_KEY}"

  ; Clean in-app update sidecars in %LOCALAPPDATA%\Battlefront Eternal while
  ; leaving %APPDATA%\Battlefront Eternal (profiles, settings, saves, and
  ; server state) untouched. Skip when invoked with /updated during an upgrade.
  ${GetParameters} $R0
  ${UnStrStr} $R1 $R0 "/updated"
  StrCmp $R1 "" 0 +7
    RMDir /r "$LOCALAPPDATA\Battlefront Eternal\versions"
    RMDir /r "$LOCALAPPDATA\Battlefront Eternal\launches"
    Delete "$LOCALAPPDATA\Battlefront Eternal\state.json"
    Delete "$LOCALAPPDATA\Battlefront Eternal\update.log"
    RMDir "$LOCALAPPDATA\Battlefront Eternal"
SectionEnd
