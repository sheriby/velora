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
  ; 关掉还在跑的 Velora：窗口各自成进程（资源管理器再开一个文件就是一个进程，应用内
  ; 「新建窗口」是同一个进程的多个窗口），它们在用着 velora.exe，不关就覆盖不了。
  ; 安装器是被静默调起来的，覆盖失败只会让「更新没动静、再点一次还是失败」重演，
  ; 所以这里代为收尾：/T 连子进程一起，/F 不等窗口同意。
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /IM velora.exe /F /T'
  Pop $0
  Sleep 300
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
  ; 与安装同理：卸载也要删 velora.exe，跑着的实例先把它们收掉。
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /IM velora.exe /F /T'
  Pop $0
  Sleep 300
  Delete "$SMPROGRAMS\Velora\Velora.lnk"
  Delete "$SMPROGRAMS\Velora\卸载 Velora.lnk"
  RMDir "$SMPROGRAMS\Velora"
  Delete "$INSTDIR\velora.exe"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\velora"
  DeleteRegKey HKCU "Software\velora"
SectionEnd
