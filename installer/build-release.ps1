<#
.SYNOPSIS
  Build and package an OpenSpore release for GitHub.

.DESCRIPTION
  From a clean checkout with Rust (stable, MSVC) installed:

      powershell -NoProfile -ExecutionPolicy Bypass -File installer\build-release.ps1

  1. builds the release exe with plain `cargo build --release --locked --bin openspore`
  2. stages dist\OpenSpore-<version>\ : openspore.exe, LICENSE, README-player.txt
  3. zips the portable build: dist\OpenSpore-<version>-portable.zip (the staged
     files under OpenSpore-<version>\, plus portable.txt unless -NoPortableMarker)
  4. compiles dist\OpenSpore-<version>-setup.exe with installer\OpenSpore.iss
     when Inno Setup 6 (ISCC.exe) is found
  5. writes dist\SHA256SUMS.txt

  OpenSpore ships no game files. Every shader is embedded in the exe
  (bevy embedded_asset!) and nothing is read from assets\ at runtime, so the
  stage holds exactly the three files above; anything else (the EA-derived
  files in assets\, extracted PNG / WAV / package data) is refused.

  Inside the OpenSpore-claude helper copies never run plain cargo (it would
  rebuild the shared dependency artifacts); build there with claude-cargo.ps1
  and stage that exe with -SkipBuild -ExePath <exe>.

.PARAMETER Version
  Release version; default: the [package] version in Cargo.toml.
.PARAMETER SkipBuild
  Do not run cargo; stage -ExePath (default target\release\openspore.exe).
.PARAMETER ExePath
  The openspore.exe to stage.
.PARAMETER OutDir
  Output folder; default <repo>\dist.
.PARAMETER NoInstaller
  Skip the Inno Setup compile even when ISCC.exe is found.
.PARAMETER NoPortableMarker
  Leave portable.txt out of the zip (saves then go to %APPDATA%\OpenSpore).
.PARAMETER DryRun
  Print the plan (version, files, outputs) and write nothing.
#>
[CmdletBinding()]
param(
    [string]$Version,
    [switch]$SkipBuild,
    [string]$ExePath,
    [string]$OutDir,
    [switch]$NoInstaller,
    [switch]$NoPortableMarker,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $OutDir) { $OutDir = Join-Path $Repo 'dist' }
$OutDir = [IO.Path]::GetFullPath($OutDir)

function Fail([string]$Message) {
    Write-Host "build-release: $Message" -ForegroundColor Red
    exit 1
}

# --- version -----------------------------------------------------------------
if (-not $Version) {
    $inPackage = $false
    foreach ($line in Get-Content (Join-Path $Repo 'Cargo.toml')) {
        if ($line -match '^\s*\[(.+)\]\s*$') { $inPackage = ($Matches[1] -eq 'package'); continue }
        if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
    }
    if (-not $Version) { Fail 'no [package] version in Cargo.toml' }
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') { Fail "version '$Version' is not MAJOR.MINOR.PATCH" }

$Name = "OpenSpore-$Version"
$Stage = Join-Path $OutDir $Name
$Zip = Join-Path $OutDir "$Name-portable.zip"
$Setup = Join-Path $OutDir "$Name-setup.exe"
$Iss = Join-Path $PSScriptRoot 'OpenSpore.iss'

# The only files a release ships (destination name -> source).
if (-not $ExePath) { $ExePath = Join-Path $Repo 'target\release\openspore.exe' }
$Ship = [ordered]@{
    'openspore.exe'     = $ExePath
    'LICENSE'           = (Join-Path $Repo 'LICENSE')
    'README-player.txt' = (Join-Path $PSScriptRoot 'README-player.txt')
}
# Never shipped, whatever ends up next to the files above.
$Forbidden = '\.(package|wav|mp3|png|dds|rw4|gmdl|obj|pdb|json)$'

function Find-Iscc {
    $cands = @()
    $cmd = Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue
    if ($cmd) { $cands += $cmd.Source }
    foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, (Join-Path $env:LOCALAPPDATA 'Programs'))) {
        if ($base) { $cands += (Join-Path $base 'Inno Setup 6\ISCC.exe') }
    }
    foreach ($c in $cands) { if ($c -and (Test-Path -LiteralPath $c)) { return $c } }
    return $null
}
$Iscc = if ($NoInstaller) { $null } else { Find-Iscc }

Write-Host "OpenSpore release $Version"
Write-Host "  repo:    $Repo"
Write-Host "  build:   $(if ($SkipBuild) { "skipped, staging $ExePath" } else { 'cargo build --release --locked --bin openspore' })"
Write-Host "  stage:   $Stage"
foreach ($k in $Ship.Keys) { Write-Host "             $k  <-  $($Ship[$k])" }
Write-Host "  zip:     $Zip$(if (-not $NoPortableMarker) { ' (+ portable.txt)' })"
Write-Host "  setup:   $(if ($Iscc) { "$Setup  (ISCC: $Iscc)" } elseif ($NoInstaller) { 'skipped (-NoInstaller)' } else { 'skipped: ISCC.exe (Inno Setup 6) not found' })"
if ($DryRun) {
    Write-Host 'Dry run: nothing written.'
    exit 0
}

# --- build -------------------------------------------------------------------
if (-not $SkipBuild) {
    if ($Repo -match 'OpenSpore-claude') {
        Fail 'refusing plain cargo inside an OpenSpore-claude helper copy; build with claude-cargo.ps1 and pass -SkipBuild -ExePath <exe>'
    }
    Push-Location $Repo
    try {
        & cargo build --release --locked --bin openspore
        if ($LASTEXITCODE -ne 0) { Fail "cargo build failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
}

# --- stage -------------------------------------------------------------------
foreach ($k in $Ship.Keys) {
    if (-not (Test-Path -LiteralPath $Ship[$k] -PathType Leaf)) { Fail "missing $k ($($Ship[$k]))" }
}
New-Item -ItemType Directory -Force $OutDir | Out-Null
if (Test-Path -LiteralPath $Stage) { Remove-Item -LiteralPath $Stage -Recurse -Force }
New-Item -ItemType Directory -Force $Stage | Out-Null
foreach ($k in $Ship.Keys) { Copy-Item -LiteralPath $Ship[$k] -Destination (Join-Path $Stage $k) }

$staged = @(Get-ChildItem -LiteralPath $Stage -Recurse -File | ForEach-Object { $_.FullName.Substring($Stage.Length + 1) })
$unexpected = @($staged | Where-Object { -not $Ship.Contains($_) -or $_ -match $Forbidden })
if ($unexpected.Count -gt 0) { Fail "unexpected files staged: $($unexpected -join ', ')" }
Write-Host "Staged $($staged.Count) files in $Stage"

# --- portable zip ------------------------------------------------------------
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path -LiteralPath $Zip) { Remove-Item -LiteralPath $Zip -Force }
$archive = [System.IO.Compression.ZipFile]::Open($Zip, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($k in $Ship.Keys) {
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $archive, (Join-Path $Stage $k), "$Name/$k", [System.IO.Compression.CompressionLevel]::Optimal)
    }
    if (-not $NoPortableMarker) {
        $entry = $archive.CreateEntry("$Name/portable.txt")
        $writer = New-Object System.IO.StreamWriter($entry.Open())
        try {
            $writer.WriteLine('Portable mode: OpenSpore keeps its settings and saves in this folder')
            $writer.WriteLine('(cached files in .\cache). Delete this file to use %APPDATA%\OpenSpore instead.')
        } finally { $writer.Dispose() }
    }
} finally { $archive.Dispose() }
Write-Host "Wrote $Zip ($([math]::Round((Get-Item -LiteralPath $Zip).Length / 1MB, 1)) MB)"

# --- setup exe ---------------------------------------------------------------
$outputs = @($Zip)
if ($Iscc) {
    & $Iscc "/DAppVersion=$Version" "/DSourceDir=$Stage" "/DOutputDir=$OutDir" $Iss
    if ($LASTEXITCODE -ne 0) { Fail "ISCC failed ($LASTEXITCODE)" }
    if (-not (Test-Path -LiteralPath $Setup)) { Fail "ISCC did not produce $Setup" }
    Write-Host "Wrote $Setup"
    $outputs += $Setup
} else {
    Write-Host 'Setup exe skipped (Inno Setup 6 not found; install it or let the GitHub workflow build it).'
}

# --- checksums ---------------------------------------------------------------
$sums = foreach ($o in $outputs) {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $o -Algorithm SHA256).Hash.ToLower(), (Split-Path $o -Leaf)
}
[IO.File]::WriteAllLines((Join-Path $OutDir 'SHA256SUMS.txt'), [string[]]$sums)
Write-Host "Wrote $(Join-Path $OutDir 'SHA256SUMS.txt')"
