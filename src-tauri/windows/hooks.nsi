; NSIS hooks for the IVAR glTF Viewer installer (referenced from tauri.conf.json).
;
; The app registers itself under HKCU\...\Run when the user opts in to "Start with
; Windows" (tauri-plugin-autostart, value name = productName). Tauri's uninstaller
; knows nothing about that entry, so without this hook an uninstall leaves Windows
; trying to launch a missing exe at every login. The first-run marker is removed too,
; so a reinstall asks the autostart question again.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IVAR glTF Viewer"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "IVAR glTF Viewer"
  Delete "$APPDATA\studio.ivar.gltf-viewer\first-run-done"
  RMDir "$APPDATA\studio.ivar.gltf-viewer"
!macroend
