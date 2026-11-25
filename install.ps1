# Installs or upgrades Sticky for the current user, without the setup window. Run in PowerShell:
#   irm https://raw.githubusercontent.com/mversion/sticky/main/install.ps1 | iex
# Downloads the latest release, checks its SHA-256 and runs `sticky.exe install`, which puts it in
# %LOCALAPPDATA%\Programs\Sticky, adds the Start-menu shortcut and the Settings > Apps entry, points
# Kite at it (if Kite is installed) and starts it. Sticky then keeps itself up to date.
$ErrorActionPreference = 'Stop'
$repo = 'mversion/sticky'
$headers = @{ 'User-Agent' = 'sticky-installer' }

$rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
$exeUrl = ($rel.assets | Where-Object name -eq 'sticky.exe').browser_download_url
$shaUrl = ($rel.assets | Where-Object name -eq 'sticky.exe.sha256').browser_download_url
if (-not $exeUrl) { throw "The latest release ($($rel.tag_name)) has no sticky.exe" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) "sticky-$([guid]::NewGuid().ToString('n'))"
New-Item -ItemType Directory $tmp | Out-Null
$exe = Join-Path $tmp 'sticky.exe'
Invoke-WebRequest $exeUrl -OutFile $exe -Headers $headers
if ($shaUrl) {
    $want = ((Invoke-RestMethod $shaUrl -Headers $headers) -split '\s+')[0].ToLower()
    $got = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { Remove-Item $tmp -Recurse; throw "Checksum mismatch: expected $want, got $got" }
}
$p = Start-Process $exe -ArgumentList 'install' -Wait -PassThru
Remove-Item $tmp -Recurse -ErrorAction SilentlyContinue
if ($p.ExitCode) { throw "Install failed (exit $($p.ExitCode))" }
Write-Host "Sticky $($rel.tag_name) installed in $env:LOCALAPPDATA\Programs\Sticky. Quick add: Ctrl+Alt+N."
