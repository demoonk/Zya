param(
    [switch]$KeepSettings,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

$installDir = Join-Path $env:APPDATA 'zя'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$taskName = 'ZyaAutoStart'
$oldNames = @(
    'z.exe',
    'zя',
    'zя.exe',
    'zya',
    'z',
    'Abrakadabra',
    'abracadabra',
    'puntoswitcher',
    'Punto Switcher',
    'keyswitcher',
    'layoutswitcher'
)

if ($DryRun) {
    Write-Host 'Dry run finished. No files were changed.'
    return
}

Get-Process |
    Where-Object { $_.Path -and $_.Path -like "$installDir*" } |
    ForEach-Object {
        try {
            Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
        } catch {
        }
    }

foreach ($value in $oldNames) {
    try {
        reg delete 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v $value /f | Out-Null
    } catch {
    }

    try {
        Remove-ItemProperty -LiteralPath $runKey -Name $value -ErrorAction SilentlyContinue
    } catch {
    }
}

try {
    schtasks /delete /tn $taskName /f | Out-Null
} catch {
}

if ($KeepSettings) {
    Get-ChildItem -LiteralPath $installDir -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notlike '*.json' -and $_.Name -notlike '*.log' } |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
} else {
    Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'zя uninstalled.'