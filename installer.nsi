; Maolan Plugins Installer
; Run with: makensis.exe installer.nsi

!include "MUI2.nsh"
!include "LogicLib.nsh"

;--------------------------------
; General
;--------------------------------
Name "Maolan Plugins"
OutFile "maolan-plugins-setup.exe"
InstallDir "$LOCALAPPDATA\Common Files\CLAP"
InstallDirRegKey HKCU "Software\MaolanPlugins" "InstallDir"
RequestExecutionLevel user

;--------------------------------
; Version Info
;--------------------------------
VIProductVersion "0.0.1.0"
VIAddVersionKey "ProductName" "Maolan Plugins"
VIAddVersionKey "ProductVersion" "0.0.1"
VIAddVersionKey "FileVersion" "0.0.1"
VIAddVersionKey "FileDescription" "Maolan CLAP Audio Plugins"
VIAddVersionKey "LegalCopyright" "BSD-2-Clause"

;--------------------------------
; Interface Settings
;--------------------------------
!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

;--------------------------------
; Pages
;--------------------------------
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

;--------------------------------
; Languages
;--------------------------------
!insertmacro MUI_LANGUAGE "English"

;--------------------------------
; Installer Sections
;--------------------------------
Section "Install"
    SetOutPath "$INSTDIR"

    ; CLAP plugin DLL
    File "C:\cargo-target\x86_64-pc-windows-msvc\release\maolan_plugins.dll"

    ; VC++ Redistributable installer (bundled)
    File "..\vc_redist.x64.exe"
    ExecWait '"$INSTDIR\vc_redist.x64.exe" /install /quiet /norestart' $0
    Delete "$INSTDIR\vc_redist.x64.exe"

    ; Store installation folder
    WriteRegStr HKCU "Software\MaolanPlugins" "InstallDir" $INSTDIR

    ; Create uninstaller
    WriteUninstaller "$INSTDIR\Uninstall-Maolan-Plugins.exe"

    ; Add to Add/Remove Programs
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "DisplayName" "Maolan Plugins"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "UninstallString" "$\"$INSTDIR\Uninstall-Maolan-Plugins.exe$\""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "DisplayVersion" "0.0.1"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "Publisher" "Maolan Team"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins" \
        "NoRepair" 1

    ; Create Start Menu shortcut
    CreateDirectory "$SMPROGRAMS\Maolan Plugins"
    CreateShortcut "$SMPROGRAMS\Maolan Plugins\Uninstall.lnk" "$INSTDIR\Uninstall-Maolan-Plugins.exe"
SectionEnd

;--------------------------------
; Uninstaller Section
;--------------------------------
Section "Uninstall"
    Delete "$INSTDIR\maolan_plugins.dll"
    Delete "$INSTDIR\Uninstall-Maolan-Plugins.exe"

    Delete "$SMPROGRAMS\Maolan Plugins\Uninstall.lnk"
    RMDir "$SMPROGRAMS\Maolan Plugins"

    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MaolanPlugins"
    DeleteRegKey HKCU "Software\MaolanPlugins"

    RMDir "$INSTDIR"
SectionEnd
