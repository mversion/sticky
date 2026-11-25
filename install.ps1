# Installs or upgrades Sticky for the current user. Run in PowerShell:
#   irm https://raw.githubusercontent.com/mversion/sticky/main/install.ps1 | iex
# Puts sticky.exe in %LOCALAPPDATA%\Programs\Sticky (or -Dir), adds a Start-menu shortcut and
# starts it. Sticky then keeps itself up to date from the GitHub releases.
param([string]$Dir = (Join-Path $env:LOCALAPPDATA 'Programs\Sticky'))
$ErrorActionPreference = 'Stop'
$repo = 'mversion/sticky'
$headers = @{ 'User-Agent' = 'sticky-installer' }

$rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
$exeUrl = ($rel.assets | Where-Object name -eq 'sticky.exe').browser_download_url
$shaUrl = ($rel.assets | Where-Object name -eq 'sticky.exe.sha256').browser_download_url
if (-not $exeUrl) { throw "The latest release ($($rel.tag_name)) has no sticky.exe" }

New-Item -ItemType Directory -Force $Dir | Out-Null
$tmp = Join-Path $Dir 'sticky.download'
Invoke-WebRequest $exeUrl -OutFile $tmp -Headers $headers
if ($shaUrl) {
    $want = ((Invoke-RestMethod $shaUrl -Headers $headers) -split '\s+')[0].ToLower()
    $got = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { Remove-Item $tmp; throw "Checksum mismatch: expected $want, got $got" }
}

# stop a running copy from this folder, then swap the exe in
Get-Process sticky -ErrorAction SilentlyContinue | Where-Object { $_.Path -and (Split-Path $_.Path) -eq $Dir } | Stop-Process -Force
Start-Sleep -Milliseconds 400
$exe = Join-Path $Dir 'sticky.exe'
Move-Item $tmp $exe -Force

$programs = [Environment]::GetFolderPath('Programs')
$ws = New-Object -ComObject WScript.Shell
$lnk = $ws.CreateShortcut((Join-Path $programs 'Sticky.lnk'))
$lnk.TargetPath = $exe
$lnk.Arguments = 'open'
$lnk.WorkingDirectory = $Dir
$lnk.Description = 'Sticky notes, to-dos and reminders'
$lnk.Save()

Start-Process $exe -ArgumentList 'open'
Write-Host "Sticky $($rel.tag_name) installed in $Dir. Quick add: Ctrl+Alt+N."
if (Test-Path (Join-Path $env:APPDATA 'Kite\settings.json')) {
    Write-Host "Kite found: run '$exe kite' (then restart Kite) to add the 'p' keyword."
}
