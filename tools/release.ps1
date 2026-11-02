# Builds the release and lays out what goes on GitHub: dist\ gets the two exes, a zip of them with
# the README, and SHA256SUMS.txt (the updater checks the exes against it before swapping them).
# It prints the `gh release create` line and stops there: publishing is yours.
# Usage: powershell -File tools\release.ps1 [-Profile quick]  (quick: try the layout without LTO)
param([string]$Profile = 'release')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# A release that doesn't know its repository can never find the next one: its users would have to
# reinstall by hand to get updates at all.
$repo = (Select-String -Path src\update.rs -Pattern 'pub const REPO: &str = "(.*)";').Matches[0].Groups[1].Value
if (-not $repo) {
    if ($Profile -eq 'release') { throw 'REPO in src\update.rs is empty: name the GitHub repository ("owner/name") before a release.' }
    Write-Warning 'REPO in src\update.rs is empty: fine for trying the layout, not for a release.'
}
# The source paths in panic messages, remapped out of the user's profile (see make-setup.ps1).
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
$env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$cargoHome=cargo", "--remap-path-prefix=$root=.") -join [char]0x1f
cargo build --profile $Profile --bin pebbly --bin pebbly-settings
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
# .cargo\config.toml keeps the build out of this (deep) folder.
$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory

$dist = Join-Path $root 'dist'
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory $dist | Out-Null
$exes = 'pebbly.exe', 'pebbly-settings.exe'
foreach ($exe in $exes) { Copy-Item (Join-Path $target "$Profile\$exe") $dist }

# No fonts: the pill draws with Segoe UI Variable then, as on the machine it was made on
# (One UI Sans can't be shared; the settings window carries Inter itself).
$zip = "Pebbly-$version.zip"
$packed = @($exes | ForEach-Object { Join-Path $dist $_ }) + (Join-Path $root 'README.md')
Compress-Archive -Path $packed -DestinationPath (Join-Path $dist $zip)

$sums = foreach ($f in $exes + $zip) {
    '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 (Join-Path $dist $f)).Hash.ToLower(), $f
}
[IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS.txt'), ($sums -join "`n") + "`n")

Write-Host "dist\ is ready for v$version. To publish:"
Write-Host "gh release create v$version dist\$zip dist\pebbly.exe dist\pebbly-settings.exe dist\SHA256SUMS.txt --title `"Pebbly $version`" --notes `"`""
