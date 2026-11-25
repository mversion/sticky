# Publishes a new version: .\scripts\release.ps1 0.2.0
# Sets the version in Cargo.toml, runs the tests, commits, tags v<version> and pushes. GitHub
# Actions then builds the release and every running Sticky updates itself within 12 hours
# (or straight away from the tray: Check for updates).
param([Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)

if (git status --porcelain) { throw 'Commit or stash your changes first.' }
$toml = Get-Content Cargo.toml -Raw
# no $ anchor: with CRLF line endings it never matches and the version silently stays put
$toml = [regex]::Replace($toml, '(?m)^version = "[^"]*"', "version = `"$Version`"", 1)
Set-Content Cargo.toml $toml -NoNewline
if ((Select-String -Path Cargo.toml -Pattern "^version = `"$([regex]::Escape($Version))`"" -Quiet) -ne $true) { throw 'Could not set the version in Cargo.toml.' }

cargo test
if ($LASTEXITCODE) { git checkout Cargo.toml; throw 'Tests failed.' }
cargo build --release
if ($LASTEXITCODE) { git checkout Cargo.toml; throw 'Build failed.' }

git add Cargo.toml Cargo.lock
git commit -m "Release v$Version"
git tag "v$Version"
git push
git push origin "v$Version"
Write-Host "Pushed v$Version. Watch the build: gh run watch --repo mversion/sticky"
