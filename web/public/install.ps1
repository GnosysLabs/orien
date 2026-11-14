# Installs orien, the program for the Orien cryptocurrency, on Windows.
# In PowerShell:
#
#   irm https://orien.network/install.ps1 | iex
#
# It downloads the Windows build, checks it against the published checksum,
# installs it into %USERPROFILE%\.orien\bin and adds that folder to your PATH;
# set ORIEN_NO_MODIFY_PATH=1 to skip that.
#
# ORIEN_HOME changes where it installs; ORIEN_BASE where it downloads from.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Base = if ($env:ORIEN_BASE) { $env:ORIEN_BASE } else { 'https://orien.network' }
$HomeDir = if ($env:ORIEN_HOME) { $env:ORIEN_HOME } else { Join-Path $env:USERPROFILE '.orien' }
$BinDir = Join-Path $HomeDir 'bin'

function Fail($message) {
    Write-Host "orien installer: $message" -ForegroundColor Red
    throw $message
}

if (-not [Environment]::Is64BitOperatingSystem) { Fail 'orien needs 64-bit Windows' }

$version = (Invoke-RestMethod "$Base/downloads/latest").Trim()
$file = "orien-$version-windows-x86_64.zip"
Write-Host "installing orien $version for Windows"

$work = Join-Path ([IO.Path]::GetTempPath()) ("orien-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work $file
    Invoke-WebRequest "$Base/downloads/$file" -OutFile $zip -UseBasicParsing
    $sums = (Invoke-WebRequest "$Base/downloads/SHA256SUMS" -UseBasicParsing).Content
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }

    # A damaged or altered download is refused.
    $line = ($sums -split "`n") | Where-Object { $_.Trim().EndsWith(" $file") } | Select-Object -First 1
    if (-not $line) { Fail "no published checksum for $file" }
    $expected = ($line.Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) { Fail 'the download does not match its published checksum; nothing was installed' }

    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Expand-Archive -Path $zip -DestinationPath $work -Force
    Move-Item -Force (Join-Path $work 'orien.exe') (Join-Path $BinDir 'orien.exe')

    # The network's starting file, when one is published.
    try {
        Invoke-WebRequest "$Base/network/genesis.json" -OutFile (Join-Path $HomeDir 'genesis.json') -UseBasicParsing
    } catch { }
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

# Make `orien` findable from new terminals.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($env:ORIEN_NO_MODIFY_PATH -ne '1' -and -not (($userPath -split ';') -contains $BinDir)) {
    $newPath = if ($userPath) { "$userPath;$BinDir" } else { $BinDir }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "added $BinDir to your PATH"
}
$env:Path = "$env:Path;$BinDir"

Write-Host ''
Write-Host ("installed: " + (& (Join-Path $BinDir 'orien.exe') --version))
Write-Host 'open a new terminal, then try: orien --help'
