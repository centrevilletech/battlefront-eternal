; Uninstalling also removes the updates the app installed for this user
; (electron/update/state.js: %LOCALAPPDATA%\Battlefront Eternal). Game data,
; settings and identities (%APPDATA%\Battlefront Eternal) stay. A newer
; installer runs this uninstaller too, as an update; the updates stay then.
!macro customUnInstall
  ${ifNot} ${isUpdated}
    RMDir /r "$LOCALAPPDATA\Battlefront Eternal\versions"
    RMDir /r "$LOCALAPPDATA\Battlefront Eternal\launches"
    Delete "$LOCALAPPDATA\Battlefront Eternal\state.json"
    Delete "$LOCALAPPDATA\Battlefront Eternal\update.log"
    RMDir "$LOCALAPPDATA\Battlefront Eternal"
  ${endIf}
!macroend
