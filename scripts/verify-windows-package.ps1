$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$config = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$releaseDir = Join-Path $repoRoot '.tmp/releases'
$feed = Get-Content -LiteralPath (Join-Path $releaseDir 'releases.win-x64.json') -Raw | ConvertFrom-Json
$asset = @($feed.Assets | Where-Object { $_.Type -eq 'Full' -and $_.Version -eq $config.version })
if ($asset.Count -ne 1 -or $asset[0].PackageId -ne $config.identifier) { throw 'Missing compatible full update in release feed.' }
$asset = $asset[0]
if ([IO.Path]::GetFileName($asset.FileName) -ne $asset.FileName) { throw 'Invalid package filename.' }
$package = Join-Path $releaseDir $asset.FileName
if ((Get-Item -LiteralPath $package).Length -ne $asset.Size) { throw 'Incorrect package size in feed.' }
$stream = [IO.File]::OpenRead($package)
$hasher = [Security.Cryptography.SHA256]::Create()
try { $checksum = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '') }
finally { $stream.Dispose(); $hasher.Dispose() }
if ($checksum -ne $asset.SHA256) { throw 'Incorrect package checksum in feed.' }
if (@(Get-ChildItem -LiteralPath $releaseDir -Filter '*Setup.exe').Count -ne 1) { throw 'Expected one Windows setup executable.' }
if (@(Get-ChildItem -LiteralPath $releaseDir -Filter '*Portable*').Count -ne 0) { throw 'Unexpected portable distribution.' }

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($package)
try {
    if (-not $archive.GetEntry('lib/app/sparkle.exe')) { throw 'Sparkle executable is missing.' }
    foreach ($file in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'licenses/Apache-2.0.txt', 'licenses/dependencies.json')) {
        $entry = $archive.GetEntry("lib/app/$file")
        if (-not $entry) { throw "Missing packaged license: $file" }
        $reader = [IO.StreamReader]::new($entry.Open())
        try { $actual = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $expected = [IO.File]::ReadAllText((Join-Path $repoRoot $file))
        if ($actual -cne $expected) { throw "Stale packaged license: $file" }
    }
    $manifestEntry = @($archive.Entries | Where-Object { $_.FullName -like '*.nuspec' })[0]
    $reader = [IO.StreamReader]::new($manifestEntry.Open())
    try { $manifestText = $reader.ReadToEnd() } finally { $reader.Dispose() }
    [xml]$manifest = $manifestText
    if ($manifest.package.metadata.shortcutLocations -ne 'StartMenuRoot') { throw 'Installer must create only a Start menu shortcut.' }
    if ($manifest.package.metadata.id -ne $config.identifier) { throw 'Installer app ID does not match Sparkle.' }
    if ($manifest.package.metadata.version -ne $config.version) { throw 'Package version does not match Sparkle.' }
    if ($manifest.package.metadata.channel -ne 'win-x64' -or $manifest.package.metadata.rid -ne 'win-x64') { throw 'Incorrect Windows update channel or architecture.' }
    if ($manifest.package.metadata.runtimeDependencies -ne 'webview2') { throw 'Missing WebView2 bootstrap configuration.' }
    if ($manifest.package.metadata.shortcutAumid -ne $config.identifier) { throw 'Incorrect shortcut application ID.' }
    Write-Output "Verified setup, update feed, package checksum, Start menu shortcut, and bundled licenses for Sparkle $($config.version)."
} finally { $archive.Dispose() }
