# Run after `scoop install` of bucket/sigstore-shell-cli.json: the first real run of the Windows exe.
$ErrorActionPreference = 'Stop'

$manifest = Get-Content (Join-Path $PSScriptRoot '..\bucket\sigstore-shell-cli.json') -Raw | ConvertFrom-Json

$line = (& sigstore-shell-cli --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw "sigstore-shell-cli --version exited with $LASTEXITCODE" }
Write-Host $line

if ($line -notmatch '^sigstore-shell-cli (\S+)') { throw "Unexpected --version output: $line" }
$version = $Matches[1]

if ($line -match '^sigstore-shell-cli (\S+) \(commit ([0-9a-f]{40})\)$') {
    if ($version -ne $manifest.version) {
        throw "--version says $version, the manifest says $($manifest.version)"
    }
} else {
    throw "--version has no commit, so the manifest version $($manifest.version) cannot be compared with it: $line"
}

# The exe checks itself: it carries its repo in the version resource and the release attests it.
$exe = Join-Path (scoop prefix sigstore-shell-cli) 'sigstore-shell-cli.exe'
$env:GH_TOKEN = $env:GITHUB_TOKEN
& $exe verify $exe
if ($LASTEXITCODE -ne 0) { throw "sigstore-shell-cli verify exited with $LASTEXITCODE" }
