# Builds windhawk_setup.exe and windhawk_setup_offline.exe for this fork: the official,
# signed Windhawk installers of the same version, rebuilt with this build's
# windhawk-ui.exe, windhawk-core.dll and windhawk-cli.exe in place of the official ones.
# The result installs like the official setup (same pages, service, shortcuts,
# uninstaller) - no second installer and no files swapped afterwards.
#
#   .\build-installers.ps1 -FilesDir <folder with the three built files>
#
# Needs Python 3 and 7-Zip.

param(
    [Parameter(Mandatory)] [string] $FilesDir,
    [string] $OutDir = (Join-Path $PSScriptRoot 'out'),
    [string] $OfficialVersion = '2.0.0-alpha.8'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Files = @('windhawk-ui.exe', 'windhawk-core.dll', 'windhawk-cli.exe')
$SevenZip = @((Get-Command 7z -ErrorAction SilentlyContinue).Source, "$env:ProgramFiles\7-Zip\7z.exe") |
    Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $SevenZip) { throw '7-Zip is needed (7z.exe).' }
$Python = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $Python) { throw 'Python 3 is needed.' }
foreach ($f in $Files) { if (-not (Test-Path (Join-Path $FilesDir $f))) { throw "$FilesDir has no $f." } }

$work = Join-Path ([IO.Path]::GetTempPath()) "windhawk-installer-build-$PID"
$new = Join-Path $work 'new'
New-Item -ItemType Directory -Force -Path $new, $OutDir | Out-Null
foreach ($f in $Files) { Copy-Item (Join-Path $FilesDir $f) $new }

try {
    foreach ($setup in 'windhawk_setup.exe', 'windhawk_setup_offline.exe') {
        $official = Join-Path $work "official-$setup"
        $url = "https://github.com/ramensoftware/windhawk/releases/download/$OfficialVersion/$setup"
        Write-Host "Downloading $url"
        Invoke-WebRequest -Uri $url -OutFile $official -UseBasicParsing
        $sig = Get-AuthenticodeSignature $official
        if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'Michael Maltsev') {
            throw "$setup from the official release is not validly signed ($($sig.Status))."
        }

        # The official copies of the three files (top level of the installer only).
        $orig = Join-Path $work "orig-$setup"
        & $SevenZip x -y -r- "-o$orig" $official @Files | Out-Null
        foreach ($f in $Files) { if (-not (Test-Path (Join-Path $orig $f))) { throw "$setup has no $f." } }

        $out = Join-Path $OutDir $setup
        & $Python -I (Join-Path $PSScriptRoot 'nsis_repack.py') $official $out $new $orig
        if ($LASTEXITCODE) { throw "Rebuilding $setup failed." }

        # Check: 7-Zip reads every file back, and only the three files differ from the official ones.
        & $SevenZip t $out | Out-Null
        if ($LASTEXITCODE) { throw "7-Zip cannot read the rebuilt $setup." }
        $check = Join-Path $work "check-$setup"
        & $SevenZip x -y -r- "-o$check" $out @Files | Out-Null
        foreach ($f in $Files) {
            if ((Get-FileHash (Join-Path $check $f)).Hash -ne (Get-FileHash (Join-Path $new $f)).Hash) {
                throw "The rebuilt $setup does not contain this build's $f."
            }
        }
    }

    Push-Location $OutDir
    try {
        Get-ChildItem 'windhawk_setup*.exe' | Sort-Object Name | ForEach-Object {
            '{0} *{1}' -f (Get-FileHash $_.Name).Hash.ToLower(), $_.Name
        } | Set-Content 'SHA256SUMS.txt' -Encoding ascii
    } finally { Pop-Location }
    Write-Host "`nDone: $OutDir"
    Get-Content (Join-Path $OutDir 'SHA256SUMS.txt')
}
finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
