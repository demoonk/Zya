# One-line install:
# powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex"

param(
    [switch]$NoStart,
    [switch]$StopPunto
)

$ErrorActionPreference = 'Stop'

$Repo = 'demoonk/Zya'
$AppDir = Join-Path $env:APPDATA 'zя'
$AppExe = Join-Path $AppDir 'zя.exe'
$ConfigFile = Join-Path $AppDir 'config.json'
$LogFile = Join-Path $AppDir 'diario.log'
$ReleaseUrl = "https://github.com/$Repo/releases/latest/download/z.exe"

function Download-App([string] $url, [string] $destination) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null

    try {
        Invoke-WebRequest -Uri $url -OutFile $destination -UserAgent 'Zya-Installer' -TimeoutSec 90
        return Test-Path -LiteralPath $destination
    } catch {
        Write-Host "Invoke-WebRequest failed: $($_.Exception.Message)"
    }

    if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
        try {
            & curl.exe -L --fail --max-time 90 --output $destination $url
            return Test-Path -LiteralPath $destination
        } catch {
            Write-Host "curl download failed: $($_.Exception.Message)"
        }
    }

    return $false
}

$puntoProcesses = Get-Process | Where-Object { $_.Name -match 'punto|puntoswitcher' }

if ($puntoProcesses.Count -gt 0) {
    Write-Host 'Punto Switcher is running:'
    foreach ($p in $puntoProcesses) {
        Write-Host "  $($p.Name) PID $($p.Id)"
    }

    if ($StopPunto) {
        foreach ($p in $puntoProcesses) {
            try {
                Stop-Process -Id $p.Id -Force
            } catch {
                Write-Host "Could not stop $($p.Name): $($_.Exception.Message)"
            }
        }
        Write-Host 'Punto Switcher was stopped.'
    } else {
        Write-Host 'ZЯ can conflict with Punto Switcher. Run with -StopPunto to close it automatically.'
    }
}

New-Item -ItemType Directory -Force -Path $AppDir | Out-Null

Get-Process |
    Where-Object {
        $_.Path -and (
            $_.Path -like "$AppDir*"
        )
    } |
    ForEach-Object {
        try {
            Stop-Process -Id $_.Id -Force
        } catch {
        }
    }

if (-not (Download-App $ReleaseUrl $AppExe)) {
    throw "Could not download $ReleaseUrl"
}

if (-not (Test-Path -LiteralPath $ConfigFile)) {
    $config = @'
{
  "margin_unknown": 2.5,
  "margin_known": 1.0,
  "word_bonus": 20.0,
  "plausibility": 0.0,
  "min_len": 3,
  "protect_code": true,
  "hotkey_undo": "alt",
  "hotkey_toggle": "ctrl+alt+p",
  "hotkey_last": "break",
  "hotkey_selection": "break",
  "hotkey_upper": "ctrl+break",
  "hotkey_invert": "shift+break",
  "whitelist": [],
  "exclude_processes": [],
  "exclude_dirs": [],
  "auto_enabled": true,
  "autostart": true,
  "log_enabled": true
}
'@

    Set-Content -LiteralPath $ConfigFile -Value $config -Encoding UTF8
}

if (-not (Test-Path -LiteralPath $LogFile)) {
    Set-Content -LiteralPath $LogFile -Value '' -Encoding UTF8
}

if (-not $NoStart) {
    Start-Process -FilePath $AppExe -WindowStyle Hidden
}

Write-Host ''
Write-Host 'zя installed.'
Write-Host "App file: $AppExe"
Write-Host "Settings folder: $AppDir"
Write-Host 'Hotkeys: Break, Ctrl+Break, Shift+Break, Ctrl+Alt+P'
Write-Host 'Uninstall: Uninstall.ps1'
