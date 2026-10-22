; devstribution installer hooks (Tauri NSIS template).

; When the user asks to delete app data, also remove the apps' secrets: they live in Windows
; Credential Manager, outside the folders the uninstaller deletes. Runs before the files go.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --forget-secrets'
  ${EndIf}
!macroend
