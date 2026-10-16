!ifndef VELORA_VERSION
  !define VELORA_VERSION "0.1.0"
!endif
!ifndef VELORA_PRODUCT_VERSION
  !define VELORA_PRODUCT_VERSION "${VELORA_VERSION}.0"
!endif
!ifndef REPO_ROOT
  !define REPO_ROOT ".."
!endif

Unicode true
Name "Velora"
OutFile "${REPO_ROOT}\dist\velora-${VELORA_VERSION}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Velora"
InstallDirRegKey HKCU "Software\velora" "InstallDir"
RequestExecutionLevel user
Icon "${REPO_ROOT}\assets\icon\velora.ico"
UninstallIcon "${REPO_ROOT}\assets\icon\velora.ico"
VIProductVersion "${VELORA_PRODUCT_VERSION}"
VIAddVersionKey "FileVersion" "${VELORA_VERSION}"
VIAddVersionKey "ProductName" "Velora"
VIAddVersionKey "ProductVersion" "${VELORA_VERSION}"
VIAddVersionKey "FileDescription" "Velora Markdown Editor"
VIAddVersionKey "LegalCopyright" "Copyright (c) velora contributors"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "安装 Velora" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "${REPO_ROOT}\target\release\velora.exe"
  File "${REPO_ROOT}\LICENSE-APACHE"

  WriteRegStr HKCU "Software\velora" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora" "DisplayName" "Velora"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora" "DisplayVersion" "${VELORA_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora" "Publisher" "Velora contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora" "UninstallString" '"$INSTDIR\Uninstall.exe"'

  CreateDirectory "$SMPROGRAMS\Velora"
  CreateShortcut "$SMPROGRAMS\Velora\Velora.lnk" "$INSTDIR\velora.exe"
  CreateShortcut "$SMPROGRAMS\Velora\卸载 Velora.lnk" "$INSTDIR\Uninstall.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  ; 卸载器没有应用层帮它先请一遍，所以自己请：不带 /F 的 taskkill 只发 WM_CLOSE，
  ; 未保存的窗口会弹出自己的提示。还留着就不继续删——硬删会把用户正在编辑的内容一起
  ; 带走，而应用已经要卸载了，恢复快照也没人去开。
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /IM velora.exe /T'
  Pop $0
  Sleep 300
  Delete "$SMPROGRAMS\Velora\Velora.lnk"
  Delete "$SMPROGRAMS\Velora\卸载 Velora.lnk"
  RMDir "$SMPROGRAMS\Velora"
  Delete "$INSTDIR\velora.exe"
  ; 删不掉就是还被锁着。这时别往下走：留下的半套卸载比「先关掉再来一次」更糟。
  IfFileExists "$INSTDIR\velora.exe" 0 velora_uninstall_can_delete
    IfSilent velora_uninstall_stop
    MessageBox MB_OK|MB_ICONEXCLAMATION "还有 Velora 窗口在运行（可能有未保存的内容）。请先关闭它们，再重新卸载。"
  velora_uninstall_stop:
    Quit
  velora_uninstall_can_delete:
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora"
  DeleteRegKey HKCU "Software\velora"
SectionEnd
