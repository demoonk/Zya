param(
    [switch]$Build,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

$root = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$project = Join-Path $root 'abrakadabra'
$installDir = Join-Path $env:APPDATA 'Abrakadra'
$installedExe = Join-Path $installDir 'zya.exe'
$configDir = Join-Path $env:APPDATA 'zя'
$configFile = Join-Path $configDir 'config.json'

function Find-BuiltExecutable {
    $candidates = @(
        (Join-Path $project 'target\release\zя.exe'),
        (Join-Path $project 'target\release\zya.exe'),
        (Join-Path $project 'target\debug\zя.exe'),
        (Join-Path $project 'target\debug\zya.exe')
    )

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    return $null
}

$source = Find-BuiltExecutable

if ($Build -or -not $source) {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        Write-Host 'Trying to build zя with cargo...'
        Push-Location $project
        try {
            cargo build --release
        } catch {
            Write-Host 'cargo build failed. If you see "linker link.exe not found", install Visual Studio Build Tools with the C++ option.'
        } finally {
            Pop-Location
        }
        $source = Find-BuiltExecutable
    }
}

if (-not $source) {
    throw "No compiled zя.exe was found in abrakadabra/target/release or abrakadabra/target/debug. Run cargo build --release first, or install Visual Studio Build Tools and run this installer again."
}

Write-Host "Using: $source"

if ($DryRun) {
    Write-Host 'Dry run finished. No files were changed.'
    return
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -LiteralPath $source -Destination $installedExe -Force

New-Item -ItemType Directory -Force -Path $configDir | Out-Null

if (-not (Test-Path -LiteralPath $configFile)) {
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
    Set-Content -LiteralPath $configFile -Value $config -Encoding UTF8
}

Get-Process |
    Where-Object { $_.Path -and $_.Path -like "$installDir*" } |
    ForEach-Object {
        try {
            Stop-Process -Id $_.Id -Force
        } catch {
        }
    }

try {
    & $installedExe install | Out-Null
} catch {
    Write-Host 'Built-in install command failed; continuing with the copied executable.'
}

$finalExe = Join-Path $configDir 'zя.exe'
$runExe = if (Test-Path -LiteralPath $finalExe) { $finalExe } else { $installedExe }

Start-Process -FilePath $runExe -ArgumentList 'run', 'headless' -WindowStyle Hidden

Write-Host ''
Write-Host 'zя installed.'
Write-Host "Copy in config folder: $finalExe"
Write-Host "Installer copy: $installedExe"
Write-Host 'Autostart: yes'
Write-Host 'Hotkeys: Break, Ctrl+Break, Shift+Break, Ctrl+Alt+P'
Write-Host 'Uninstall: Uninstall.ps1'
