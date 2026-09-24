!ifndef MAKSHER_VERSION
  !define MAKSHER_VERSION "0.1.0"
!endif
!ifndef REPO_ROOT
  !define REPO_ROOT ".."
!endif

Unicode true
Name "maksher"
OutFile "${REPO_ROOT}\dist\maksher-${MAKSHER_VERSION}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\maksher"
InstallDirRegKey HKCU "Software\maksher" "InstallDir"
RequestExecutionLevel user
Icon "${REPO_ROOT}\assets\icon\maksher.ico"
UninstallIcon "${REPO_ROOT}\assets\icon\maksher.ico"
VIProductVersion "${MAKSHER_VERSION}.0"
VIAddVersionKey "FileVersion" "${MAKSHER_VERSION}"
VIAddVersionKey "ProductName" "maksher"
VIAddVersionKey "ProductVersion" "${MAKSHER_VERSION}"
VIAddVersionKey "FileDescription" "maksher Markdown Editor"
VIAddVersionKey "LegalCopyright" "Copyright (c) maksher contributors"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "安装 maksher" SEC_MAIN
  SetOutPath "$INSTDIR"
  File "${REPO_ROOT}\target\x86_64-pc-windows-gnu\fastdev\maksher.exe"
  File "${REPO_ROOT}\LICENSE-APACHE"

  WriteRegStr HKCU "Software\maksher" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher" "DisplayName" "maksher"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher" "DisplayVersion" "${MAKSHER_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher" "Publisher" "maksher contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher" "UninstallString" '"$INSTDIR\Uninstall.exe"'

  CreateDirectory "$SMPROGRAMS\maksher"
  CreateShortcut "$SMPROGRAMS\maksher\maksher.lnk" "$INSTDIR\maksher.exe"
  CreateShortcut "$SMPROGRAMS\maksher\卸载 maksher.lnk" "$INSTDIR\Uninstall.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\maksher\maksher.lnk"
  Delete "$SMPROGRAMS\maksher\卸载 maksher.lnk"
  RMDir "$SMPROGRAMS\maksher"
  Delete "$INSTDIR\maksher.exe"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\maksher"
  DeleteRegKey HKCU "Software\maksher"
SectionEnd
