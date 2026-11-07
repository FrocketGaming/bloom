; Extra steps for Bloom's Windows installer.
;
; Bloom adds its own entry to the current user's Run key when it is set to start
; with Windows, so the uninstaller has to take it out, or Windows keeps looking
; for a program that is gone. An upgrade runs the old uninstaller too, with
; /UPDATE; the entry stays then, because the new version still wants it.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Bloom"
  ${EndIf}
!macroend
