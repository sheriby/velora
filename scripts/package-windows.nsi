!ifndef VELORA_VERSION
  !define VELORA_VERSION "0.1.0"
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
VIProductVersion "${VELORA_VERSION}.0"
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
  File "${REPO_ROOT}\target\x86_64-pc-windows-gnu\fastdev\velora.exe"
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
