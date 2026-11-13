<#
.SYNOPSIS
  Checks build-release.ps1 without a release build: a dry run writes nothing,
  and staging + zip of a given exe produce exactly the shipped files.

      powershell -NoProfile -ExecutionPolicy Bypass -File installer\test-build-release.ps1 -ExePath target\debug\openspore.exe

  Everything is written under a fresh folder in %TEMP% and removed afterwards.
#>
param([Parameter(Mandatory = $true)][string]$ExePath)

$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'build-release.ps1'
$ExePath = (Resolve-Path $ExePath).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("openspore-release-test-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$failures = 0
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "PASS $what" } else { Write-Host "FAIL $what" -ForegroundColor Red; $script:failures++ }
}
function Run-Release([string[]]$extra) {
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $script @extra 2>&1 | Out-String
    return @{ Code = $LASTEXITCODE; Text = $out }
}

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
try {
    $version = '9.8.7'
    $name = "OpenSpore-$version"

    # 1. Dry run: prints the plan, writes nothing.
    $dry = Join-Path $tmp 'dry'
    $r = Run-Release @('-Version', $version, '-SkipBuild', '-ExePath', $ExePath, '-OutDir', $dry, '-DryRun')
    Check ($r.Code -eq 0) 'dry run exits 0'
    Check ($r.Text -match 'Dry run: nothing written') 'dry run says it wrote nothing'
    Check ($r.Text -match 'README-player.txt') 'dry run lists the shipped files'
    Check (-not (Test-Path $dry)) 'dry run created no output folder'

    # 2. Stage + portable zip from the given exe (no installer).
    $out = Join-Path $tmp 'dist'
    $r = Run-Release @('-Version', $version, '-SkipBuild', '-ExePath', $ExePath, '-OutDir', $out, '-NoInstaller')
    Write-Host $r.Text
    Check ($r.Code -eq 0) 'staging run exits 0'
    $stage = Join-Path $out $name
    $files = @(Get-ChildItem -LiteralPath $stage -Recurse -File | ForEach-Object { $_.Name } | Sort-Object)
    Check (($files -join ',') -eq 'LICENSE,openspore.exe,README-player.txt') "stage holds exactly exe, LICENSE, README-player.txt ($($files -join ', '))"
    Check ((Get-FileHash (Join-Path $stage 'openspore.exe')).Hash -eq (Get-FileHash $ExePath).Hash) 'staged exe is the given exe'
    Check ((Get-Content (Join-Path $stage 'LICENSE') -TotalCount 3 | Out-String) -match 'GNU GENERAL PUBLIC LICENSE') 'LICENSE is the GPL'
    $zipPath = Join-Path $out "$name-portable.zip"
    Check (Test-Path $zipPath) 'portable zip written'
    $zip = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try { $entries = @($zip.Entries | ForEach-Object { $_.FullName } | Sort-Object) } finally { $zip.Dispose() }
    $expected = @("$name/LICENSE", "$name/openspore.exe", "$name/portable.txt", "$name/README-player.txt") | Sort-Object
    Check (($entries -join ',') -eq ($expected -join ',')) "zip entries: $($entries -join ', ')"
    Check (@($entries | Where-Object { $_ -match '\.(wav|png|package|mp3|json|obj)$' }).Count -eq 0) 'zip holds no EA-derived file types'
    $sums = Get-Content (Join-Path $out 'SHA256SUMS.txt')
    $zipHash = (Get-FileHash $zipPath).Hash.ToLower()
    Check (($sums -join "`n") -match [regex]::Escape("$zipHash  $name-portable.zip")) 'SHA256SUMS lists the zip'
    Check (-not (Test-Path (Join-Path $out "$name-setup.exe"))) 'no setup exe with -NoInstaller'

    # 3. -NoPortableMarker leaves portable.txt out; a re-run replaces the old output.
    $r = Run-Release @('-Version', $version, '-SkipBuild', '-ExePath', $ExePath, '-OutDir', $out, '-NoInstaller', '-NoPortableMarker')
    Check ($r.Code -eq 0) 'second run exits 0'
    $zip = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try { $entries = @($zip.Entries | ForEach-Object { $_.FullName }) } finally { $zip.Dispose() }
    Check ($entries.Count -eq 3 -and -not ($entries -contains "$name/portable.txt")) 'zip without portable.txt'

    # 4. A missing input fails instead of shipping an incomplete build.
    $r = Run-Release @('-Version', $version, '-SkipBuild', '-ExePath', (Join-Path $tmp 'nope.exe'), '-OutDir', $out, '-NoInstaller')
    Check ($r.Code -ne 0 -and $r.Text -match 'missing openspore.exe') 'missing exe fails'
    $r = Run-Release @('-Version', 'v1', '-SkipBuild', '-ExePath', $ExePath, '-OutDir', $out, '-DryRun')
    Check ($r.Code -ne 0) 'malformed version fails'
} finally {
    if (Test-Path $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
}
if ($failures -gt 0) { Write-Host "$failures check(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'All build-release checks passed.'
