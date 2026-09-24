<#
.SYNOPSIS
    Build the Vadadee Berry Windows installer (.exe, WiX Toolset v4).

.DESCRIPTION
    Uses binaries already built by the desktop release job
    (make_desktop_release.sh / cargo build --release), stages them with the
    shared icon, and builds VadadeeBerry.wxs with the WiX v4 .NET tool
    (installed on demand; needs network on first use).

.PARAMETER BinDir
    Directory containing vadadee-berry.exe and vadadee-mcp-stdio.exe.
    Defaults to target\release.

.PARAMETER OutDir
    Where to write the installer. Defaults to release\.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File packaging\windows\build_installer.ps1
#>
[CmdletBinding()]
param(
    [string]$BinDir = (Join-Path $PSScriptRoot '..\..\target\release'),
    [string]$OutDir = (Join-Path $PSScriptRoot '..\..\release')
)

$ErrorActionPreference = 'Stop'

$studio = Join-Path $BinDir 'vadadee-berry.exe'
$mcp = Join-Path $BinDir 'vadadee-mcp-stdio.exe'
foreach ($b in @($studio, $mcp)) {
    if (-not (Test-Path $b)) {
        Write-Error "Missing binary: $b (build it first: cargo build --release --bin vadadee-berry --bin vadadee-mcp-stdio)"
        exit 1
    }
}

# Version from Cargo.toml (single source of truth, like the bash scripts).
$cargo = Get-Content (Join-Path $PSScriptRoot '..\..\Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    Write-Error 'Could not parse version from Cargo.toml'
    exit 1
}
$version = $Matches[1]
Write-Host "Version: $version"

$wxs = Join-Path $PSScriptRoot 'VadadeeBerry.wxs'
$ico = Join-Path $PSScriptRoot 'vadadee-berry.ico'
foreach ($f in @($wxs, $ico)) {
    if (-not (Test-Path $f)) {
        Write-Error "Missing packaging file: $f"
        exit 1
    }
}

# WiX v4 as a dotnet tool (windows-latest runners ship dotnet).
$wix = (Get-Command wix -ErrorAction SilentlyContinue)?.Source
if (-not $wix) {
    Write-Host 'Installing WiX Toolset v4 (needs network)...'
    dotnet tool install --global wix
    $wix = (Get-Command wix -ErrorAction SilentlyContinue)?.Source
}
if (-not $wix) {
    Write-Error 'WiX v4 CLI not found after install. Ensure dotnet is on PATH.'
    exit 1
}
& $wix --version

$outName = "vadadee-berry-$version-windows-x86_64-setup.exe"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$outPath = Join-Path $OutDir $outName
if (Test-Path $outPath) { Remove-Item $outPath -Force }

# Build from the packaging dir so the relative icon source resolves.
Push-Location (Join-Path $PSScriptRoot '.')
try {
    & $wix build VadadeeBerry.wxs `
        "-dVersion=$version" `
        "-dBinDir=$BinDir" `
        -o "$outPath"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

if (-not (Test-Path $outPath)) {
    Write-Error 'WiX build produced no installer.'
    exit 1
}
$hash = (Get-FileHash $outPath -Algorithm SHA256).Hash.ToLower()
"$hash  $outName" | Out-File -FilePath "$outPath.sha256" -Encoding ascii -NoNewline
Write-Host "Done: $outPath"
