param(
    [switch]$KeepSettings,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

$installDir = Join-Path $env:APPDATA 'Abrakadra'
$configDir = Join-Path $env:APPDATA 'zя'

if ($DryRun) {
    Write-Host 'Dry run finished. No files were changed.'
    return
}

Get-Process |
    Where-Object { $_.Path -and ($_.Path -like "$installDir*" -or $_.Path -like "$configDir*") } |
    ForEach-Object {
        try {
            Stop-Process -Id $_.Id -Force
        } catch {
        }
    }

foreach ($value in @('z.exe', 'zя', 'zya', 'z')) {
    try {
        reg delete 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v $value /f | Out-Null
    } catch {
    }
}

Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue

if (-not $KeepSettings) {
    Remove-Item -LiteralPath $configDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'zя uninstalled.'
