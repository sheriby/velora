$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $IsWindows) {
    throw '请在原生 Windows x64 环境中运行此脚本。'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Get-Content (Join-Path $repoRoot 'Cargo.toml') -Raw
$version = [regex]::Match($manifest, '(?m)^version = "([^"]+)"').Groups[1].Value
$productVersion = ($version -split '[-+]')[0] + '.0'

$rustVersion = & rustc -vV
if ($LASTEXITCODE -ne 0) { throw '无法读取 Rust 工具链信息。' }
if ($rustVersion -notcontains 'host: x86_64-pc-windows-msvc') {
    throw '请安装并启用 x86_64-pc-windows-msvc Rust 工具链。'
}

$nsisCompiler = Get-Command makensis.exe -ErrorAction SilentlyContinue
if (-not $nsisCompiler) {
    $nsisPath = Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'
    $nsisCompiler = Get-Command $nsisPath -ErrorAction SilentlyContinue
}
if (-not $nsisCompiler) { throw '未找到 makensis.exe；请安装 NSIS。' }

if (-not $env:GPUI_FXC_PATH) {
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $shaderCompiler = Get-ChildItem "$sdkRoot\*\x64\fxc.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $shaderCompiler) { throw '未找到 fxc.exe；请安装 Windows SDK。' }
    $env:GPUI_FXC_PATH = $shaderCompiler.FullName
}
if (-not (Test-Path $env:GPUI_FXC_PATH -PathType Leaf)) {
    throw "着色器编译器不存在：$env:GPUI_FXC_PATH"
}

Push-Location $repoRoot
try {
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Windows release 构建失败。' }

    $binaryPath = Join-Path $repoRoot 'target\release\velora.exe'
    $binaryBytes = [System.IO.File]::ReadAllBytes($binaryPath)
    if ($binaryBytes.Length -lt 64 -or [BitConverter]::ToUInt16($binaryBytes, 0) -ne 0x5A4D) {
        throw '产物缺少有效的 DOS 文件头。'
    }
    $peOffset = [BitConverter]::ToInt32($binaryBytes, 0x3C)
    if ($peOffset -lt 0 -or $peOffset -gt $binaryBytes.Length - 26 -or
        [BitConverter]::ToUInt32($binaryBytes, $peOffset) -ne 0x00004550 -or
        [BitConverter]::ToUInt16($binaryBytes, $peOffset + 4) -ne 0x8664 -or
        [BitConverter]::ToUInt16($binaryBytes, $peOffset + 24) -ne 0x020B) {
        throw '产物不是 Windows x64 PE32+ 可执行文件。'
    }
    # Common-Controls v6 是加载器硬依赖，缺失会使应用在启动时失败。
    if (-not [System.Text.Encoding]::ASCII.GetString($binaryBytes).Contains('Microsoft.Windows.Common-Controls')) {
        throw 'exe 未嵌入 Common-Controls manifest，拒绝打包。'
    }

    & $nsisCompiler.Source /INPUTCHARSET UTF8 "/DVELORA_VERSION=$version" "/DVELORA_PRODUCT_VERSION=$productVersion" "/DREPO_ROOT=$repoRoot" scripts/package-windows.nsi
    if ($LASTEXITCODE -ne 0) { throw 'NSIS 打包失败。' }

    $packagePath = Join-Path $repoRoot "dist\velora-$version-windows-x64-setup.exe"
    if ((Get-Item $packagePath).Length -eq 0) { throw '安装包为空。' }
    Write-Host "已生成：$packagePath"
}
finally {
    Pop-Location
}
