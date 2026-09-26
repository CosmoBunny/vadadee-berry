<#
.SYNOPSIS
    Build the Vadadee Berry Windows installer (.msi + -setup.exe Burn bundle, WiX Toolset v5).

.DESCRIPTION
    Uses binaries already built by the desktop release job
    (make_desktop_release.sh / cargo build --release), stages them with the
    shared icon, and builds VadadeeBerry.wxs with the WiX v5 .NET tool
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

# WiX v5 as a dotnet tool (windows-latest runners ship dotnet).
# Pinned to v5 on purpose: v7 gates usage behind an OSMF EULA acceptance
# step (WIX7015) that has no place in unattended CI, while v5 uses the same
# v4 wxs schema this package is authored in.
$wix = (Get-Command wix -ErrorAction SilentlyContinue)?.Source
if (-not $wix) {
    Write-Host 'Installing WiX Toolset v5 (needs network)...'
    dotnet tool install --global wix --version '5.*'
    $wix = (Get-Command wix -ErrorAction SilentlyContinue)?.Source
}
if (-not $wix) {
    Write-Error 'WiX v4 CLI not found after install. Ensure dotnet is on PATH.'
    exit 1
}
& $wix --version

# BootstrapperApplications extension (provides WixStdBA for the Bundle).
# Pinned to the installed WiX major (5.0.2): the -ext flag alone does NOT
# fetch it, and the build fails with WIX0144 without it. Installed GLOBAL
# (-g): a directory-local install is invisible to `wix build` when it runs
# from another directory (the Bundle build runs from packaging/windows).
# The list check below is the real gate (the add itself is best-effort:
# the extension may already be cached, e.g. when CI pre-installs it).
& $wix extension add -g WixToolset.Bal.wixext/5.0.2
$extList = (& $wix extension list) -join "`n"
if ($extList -notmatch 'WixToolset\.Bal\.wixext') {
    Write-Error 'Failed to install WixToolset.Bal.wixext required by VadadeeBerry.Bundle.wxs.'
    exit 1
}

$outName = "vadadee-berry-$version-windows-x86_64-setup.exe"
$msiName = "vadadee-berry-$version-windows-x86_64.msi"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$outPath = Join-Path $OutDir $outName
$msiPath = Join-Path $OutDir $msiName
if (Test-Path $outPath) { Remove-Item $outPath -Force }
if (Test-Path $msiPath) { Remove-Item $msiPath -Force }

# Two-stage WiX v5 build (output type must match the source entry type):
#   stage 1  VadadeeBerry.wxs (<Package>)        -> .msi
#   stage 2  VadadeeBerry.Bundle.wxs (<Bundle>)  -> -setup.exe wrapping the MSI
# Never rename an MSI to .exe (WIX1109/WIX0341). The -ext flag loads the
# BootstrapperApplications extension providing WixStdBA.

# Build from the packaging dir so the relative icon source resolves.
# NOTE: -d takes its value as the NEXT token (attached -dName= form is
# rejected: WIX0118).
Push-Location (Join-Path $PSScriptRoot '.')
try {
    & $wix build VadadeeBerry.wxs `
        -d "Version=$version" `
        -d "BinDir=$BinDir" `
        -o "$msiPath"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & $wix build VadadeeBerry.Bundle.wxs `
        -d "Version=$version" `
        -d "MsiPath=$msiPath" `
        -ext WixToolset.Bal.wixext `
        -o "$outPath"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

if (-not (Test-Path $msiPath)) {
    Write-Error 'WiX build produced no MSI.'
    exit 1
}
if (-not (Test-Path $outPath)) {
    Write-Error 'WiX build produced no installer.'
    exit 1
}
$msiHash = (Get-FileHash $msiPath -Algorithm SHA256).Hash.ToLower()
"$msiHash  $msiName" | Out-File -FilePath "$msiPath.sha256" -Encoding ascii -NoNewline
$hash = (Get-FileHash $outPath -Algorithm SHA256).Hash.ToLower()
"$hash  $outName" | Out-File -FilePath "$outPath.sha256" -Encoding ascii -NoNewline
Write-Host "Done: $outPath + $msiPath"
