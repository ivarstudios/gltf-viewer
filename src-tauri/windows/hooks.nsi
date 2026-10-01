; NSIS hooks for the IVAR glTF Viewer installer (referenced from tauri.conf.json).
;
; Tauri's own uninstaller already deletes HKCU\...\Run\"IVAR glTF Viewer" (the value name
; is the product name, which is why the app uses exactly that name). It does so whenever
; $UpdateMode <> 1, and a normal upgrade runs the old uninstaller WITHOUT /UPDATE (only the
; updater plugin passes it), so the entry is lost on every upgrade. The app repairs that
; itself on its next start from the preference it keeps in $APPDATA\studio.ivar.gltf-viewer
; (see src/autostart.rs), which is why nothing here must touch that folder: the uninstaller's
; "Delete app data" checkbox removes it when the user wants a full reset.
;
; What is left for this hook is the Task Manager Startup-tab override, which the template
; does not know about.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "IVAR glTF Viewer"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "IVAR glTF Viewer"
  ${EndIf}
!macroend
