# Builds what a GitHub release carries, into dist\: pebbly-setup.exe (both exes inside),
# pebbly.exe and pebbly-settings.exe (what the updater downloads) and SHA256SUMS.txt.
# Every release also carries nowbar.exe (the setup) and nowbar-settings.exe (the settings window)
# for Now Bar 0.2.0 to 0.5.0, the app's name before: their updater asks the old repository (GitHub
# redirects it) for exactly these two names, swaps them in and starts nowbar.exe, and the setup,
# started under that name, moves the copy to Pebbly (setup/src/main.rs, `bridge`).
#
#   powershell -ExecutionPolicy Bypass -File tools\make-setup.ps1 [-Jobs 2] [-Keep]
#
# The C runtime is linked statically: the exes open on a Windows without the Visual C++
# redistributable. The dist profile is release with thin LTO and stripped symbols. Its build
# (4.5 GB, used only for a release; the clone's builds get 2) is deleted at the end: the next
# release builds every dependency again. -Keep keeps it, minus the copies these builds didn't use
# (tools\sweep.py). No exe may name the user who built it: the source paths in panic messages are
# remapped, and a path left over stops the release.
param([int]$Jobs = 2, [switch]$Keep)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $version = ($meta.packages | Where-Object name -eq 'pebbly').version
    $built = Join-Path $meta.target_directory 'dist'
    $dist = Join-Path $repo 'dist'
    $payload = Join-Path $dist 'payload'

    # Panic messages carry every crate's source path: the registry's under the cargo home, the
    # build scripts' output under this folder, both in the user's profile. Encoded flags: a path
    # may hold spaces.
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C', 'target-feature=+crt-static', "--remap-path-prefix=$cargoHome=cargo", "--remap-path-prefix=$repo=.") -join [char]0x1f
    # JSON on stdout names every unit these builds use, fresh or not: the sweep keeps those.
    $messages = cargo build -j $Jobs --profile dist --bin pebbly --bin pebbly-settings --message-format=json-render-diagnostics
    if ($LASTEXITCODE) { throw 'the exes did not build' }
    New-Item -ItemType Directory -Force $payload | Out-Null
    foreach ($exe in 'pebbly.exe', 'pebbly-settings.exe') { Copy-Item (Join-Path $built $exe) $payload -Force }

    $env:PEBBLY_SETUP_PAYLOAD = $payload
    $env:PEBBLY_VERSION = $version
    $messages += cargo build -j $Jobs --profile dist -p pebbly-setup --message-format=json-render-diagnostics
    if ($LASTEXITCODE) { throw 'the setup did not build' }
    Copy-Item (Join-Path $built 'pebbly-setup.exe') $dist -Force
    foreach ($exe in 'pebbly.exe', 'pebbly-settings.exe') { Copy-Item (Join-Path $payload $exe) $dist -Force }
    Copy-Item (Join-Path $dist 'pebbly-setup.exe') (Join-Path $dist 'nowbar.exe') -Force
    Copy-Item (Join-Path $dist 'pebbly-settings.exe') (Join-Path $dist 'nowbar-settings.exe') -Force

    # The updater's format: "<sha256>  <name>" per line; Now Bar's checks its two names in it too.
    $sums = foreach ($exe in 'pebbly-setup.exe', 'pebbly.exe', 'pebbly-settings.exe', 'nowbar.exe', 'nowbar-settings.exe') {
        '{0}  {1}' -f (Get-FileHash (Join-Path $dist $exe) -Algorithm SHA256).Hash.ToLower(), $exe
    }
    Set-Content (Join-Path $dist 'SHA256SUMS.txt') $sums -Encoding ascii

    # No exe may need the Visual C++ runtime DLLs, or name the user's folder.
    $userPath = '(?i)' + [regex]::Escape($env:USERPROFILE) + '|[\\/]users[\\/]' + [regex]::Escape($env:USERNAME) + '[\\/]'
    foreach ($exe in 'pebbly-setup.exe', 'pebbly.exe', 'pebbly-settings.exe') {
        $bytes = [System.IO.File]::ReadAllBytes((Join-Path $dist $exe))
        $text = [System.Text.Encoding]::ASCII.GetString($bytes)
        if ($text -match '(?i)vcruntime140|msvcp140') { throw "$exe still imports the Visual C++ runtime" }
        if ([System.Text.Encoding]::UTF8.GetString($bytes) -match $userPath -or [System.Text.Encoding]::Unicode.GetString($bytes) -match $userPath) { throw "$exe still holds a path in $env:USERPROFILE" }
        '{0,-22} {1,6:N1} MB' -f $exe, ($bytes.Length / 1MB)
    }
    "v$version -> $dist"
    "gh release create v$version dist\pebbly-setup.exe dist\pebbly.exe dist\pebbly-settings.exe dist\nowbar.exe dist\nowbar-settings.exe dist\SHA256SUMS.txt"
    Remove-Item $payload -Recurse -Force
    if ($Keep) {
        $list = Join-Path $built 'messages.json'
        [System.IO.File]::WriteAllLines($list, [string[]]$messages)
        python (Join-Path $PSScriptRoot 'sweep.py') --profile dist --messages $list
    }
    else {
        Remove-Item $built -Recurse -Force -ErrorAction SilentlyContinue
        'the dist build was deleted'
    }
}
finally {
    Pop-Location
}
