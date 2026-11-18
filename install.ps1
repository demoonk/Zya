# One-line install:
# powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex"
#
# The installer requires administrator rights. It downloads the GitHub asset z.exe
# and installs the local resident app as %APPDATA%\zя\zя.exe.

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
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

function Test-IsAdmin {
    try {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = [Security.Principal.WindowsPrincipal] $identity
        return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    } catch {
        return $false
    }
}

if (-not (Test-IsAdmin)) {
    Write-Host 'Необходимо запустить установку от имени администратора.'
    Write-Host 'Правый клик -> "Запуск от имени администратора", или powershell -RunAs admin.'
    exit 1
}

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

function Kill-OldProcesses {
    $oldNames = @(
        'z',
        'z.exe',
        'zя',
        'zя.exe',
        'z_app',
        'z_app.exe',
        'abrakadabra',
        'abracadabra',
        'puntoswitcher',
        'punto_switcher',
        'keyswitcher',
        'layoutswitcher'
    )

    foreach ($name in $oldNames) {
        Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {
            try {
                Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
            } catch {
            }
        }
    }

    $puntoProcesses = Get-Process | Where-Object { $_.Name -match 'punto|puntoswitcher' }
    if ($puntoProcesses.Count -gt 0 -or $StopPunto) {
        foreach ($p in $puntoProcesses) {
            try {
                Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
            } catch {
            }
        }
    }

    Get-Process |
        Where-Object { $_.Path -and $_.Path.StartsWith($AppDir, [System.StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object {
            try {
                Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
            } catch {
            }
        }
}

function Remove-OldAutostart {
    if (-not (Test-Path $RunKey)) {
        return
    }

    $oldValues = @(
        'z.exe',
        'zя',
        'zya',
        'z',
        'Abrakadabra',
        'abracadabra',
        'puntoswitcher',
        'Punto Switcher',
        'keyswitcher',
        'layoutswitcher'
    )

    foreach ($value in $oldValues) {
        try {
            Remove-ItemProperty -LiteralPath $RunKey -Name $value -ErrorAction SilentlyContinue
        } catch {
        }
    }
}

Kill-OldProcesses
Remove-OldAutostart

New-Item -ItemType Directory -Force -Path $AppDir | Out-Null

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

try {
    New-ItemProperty -LiteralPath $RunKey -Name 'zя.exe' -PropertyType String -Value "`"$AppExe`" run headless" -Force | Out-Null
} catch {
    Write-Host "Не удалось добавить автозапуск: $($_.Exception.Message)"
}

if (-not $NoStart) {
    $stdoutFile = Join-Path $AppDir 'start_stdout.log'
    $stderrFile = Join-Path $AppDir 'start_stderr.log'

    try {
        $proc = Start-Process -FilePath $AppExe -ArgumentList 'run', 'headless' -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput $stdoutFile `
            -RedirectStandardError $stderrFile

        Start-Sleep -Seconds 3

        if (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue) {
            Write-Host 'zя.exe запущен: значок RU/EN должен появиться в трее.'
        } else {
            Write-Host 'zя.exe не запустился как resident app. Проверьте:'
            if (Test-Path -LiteralPath $stderrFile) {
                Get-Content -LiteralPath $stderrFile -TotalCount 20
            }
            Write-Host "Можно запустить вручную: $AppExe"
        }
    } catch {
        Write-Host "Could not start zя automatically: $($_.Exception.Message)"
        Write-Host "Можно запустить вручную: $AppExe"
    }
}

Write-Host ''
Write-Host 'zя installed.'
Write-Host "GitHub asset: z.exe"
Write-Host "Installed app: $AppExe"
Write-Host "Settings folder: $AppDir"
Write-Host 'Menu: Автосправление раскладки, Автозапуск при входе, Настройки, Выход'
Write-Host 'Hotkeys: Break, Ctrl+Break, Shift+Break, Ctrl+Alt+P'
Write-Host 'Uninstall: Uninstall.ps1'