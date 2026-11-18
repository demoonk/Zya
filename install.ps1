param(
    [switch]$Build,
    [switch]$DryRun,
    [switch]$NoDownload,
    [switch]$InstallDependencies,
    [switch]$StopPunto
)

$ErrorActionPreference = 'Stop'

$Repo = 'demoonk/Zya'
$Root = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$Project = Join-Path $Root 'abrakadabra'
$InstallDir = Join-Path $env:APPDATA 'Abrakadra'
$ConfigDir = Join-Path $env:APPDATA 'zя'
$ConfigFile = Join-Path $ConfigDir 'config.json'
$InstalledExe = Join-Path $InstallDir 'zya.exe'
$TempDir = Join-Path $env:TEMP 'zя-install'

function Invoke-GitHubJson([string] $url) {
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        return Invoke-RestMethod -Uri $url -Headers @{ 'User-Agent' = 'Zya-Installer' } -TimeoutSec 30
    } catch {
        Write-Host "Could not read GitHub API: $($_.Exception.Message)"
        return $null
    }
}

function Download-File([string] $url, [string] $destination) {
    try {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
        Invoke-WebRequest -Uri $url -OutFile $destination -UserAgent 'Zya-Installer' -TimeoutSec 90
        return Test-Path -LiteralPath $destination
    } catch {
        Write-Host "Download failed: $url"
        Write-Host "  $($_.Exception.Message)"
        return $false
    }
}

function Get-ReleaseDownloadUrl() {
    $release = Invoke-GitHubJson "https://api.github.com/repos/$Repo/releases/latest"
    if ($release -and $release.assets.Count -gt 0) {
        return $release.assets[0].browser_download_url
    }

    return "https://github.com/$Repo/releases/download/v0.1.0/zya.exe"
}

function Find-BuiltExecutable() {
    $candidates = @(
        (Join-Path $Project 'target\release\zя.exe'),
        (Join-Path $Project 'target\release\zya.exe'),
        (Join-Path $Project 'target\debug\zя.exe'),
        (Join-Path $Project 'target\debug\zya.exe')
    )

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    return $null
}

function Find-Cargo() {
    $cmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    $userCargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path -LiteralPath $userCargo) {
        return $userCargo
    }

    return $null
}

function Ensure-Cargo() {
    $cargo = Find-Cargo
    if ($cargo) {
        return $cargo
    }

    Write-Host 'Cargo/Rust is missing. Downloading rustup-init.exe...'
    $rustup = Join-Path $TempDir 'rustup-init.exe'
    if (Download-File 'https://static.rust-lang.org/dist/rustup-init.exe' $rustup) {
        Write-Host 'Installing Rust with rustup...'
        Start-Process -FilePath $rustup -ArgumentList '-y' -Wait

        $cargo = Find-Cargo
        if ($cargo) {
            return $cargo
        }
    }

    throw 'Rust/Cargo is required to build zя.'
}

function Find-MSVCLinker() {
    $cmd = Get-Command link.exe -ErrorAction SilentlyContinue
    if ($cmd) {
        return @{
            Path = $cmd.Source
            Dir = Split-Path -Parent $cmd.Source
        }
    }

    $patterns = @(
        'C:\BuildTools\VC\Tools\MSVC\*\bin\X64\link.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\*\bin\X64\link.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\*\bin\X64\link.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\MSVC\*\bin\X64\link.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC\*\bin\X64\link.exe',
        'C:\Program Files\Microsoft Visual Studio\2019\BuildTools\VC\Tools\MSVC\*\bin\X64\link.exe'
    )

    foreach ($pattern in $patterns) {
        $found = Get-ChildItem -Path $pattern -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) {
            return @{
                Path = $found.FullName
                Dir = Split-Path -Parent $found.FullName
            }
        }
    }

    return $null
}

function Ensure-MSVCLinker() {
    $link = Find-MSVCLinker
    if ($link) {
        $env:PATH = "$($link.Dir);$env:PATH"
        return
    }

    Write-Host 'MSVC linker link.exe is missing. Downloading Visual Studio Build Tools...'
    $installer = Join-Path $TempDir 'vs_BuildTools.exe'

    if (Download-File 'https://aka.ms/vs/17/release/vs_BuildTools.exe' $installer) {
        Write-Host 'Installing Visual Studio Build Tools with the C++ build workload...'

        try {
            Start-Process -FilePath $installer `
                -ArgumentList '--quiet', '--wait', '--norestart', '--nocache', `
                    '--add', 'Microsoft.VisualStudio.Workload.VCTools', `
                    '--includeRecommended' `
                -Wait
        } catch {
            Write-Host 'VS Build Tools installation failed. It may require Administrator rights.'
        }

        $link = Find-MSVCLinker
        if ($link) {
            $env:PATH = "$($link.Dir);$env:PATH"
            Write-Host 'MSVC linker found.'
        } else {
            Write-Host 'VS Build Tools installation finished, but link.exe was still not found.'
        }
    }
}

function Build-Project() {
    $cargo = Ensure-Cargo
    Ensure-MSVCLinker

    Push-Location $Project
    try {
        Write-Host 'Building zя with cargo...'
        & $cargo build --release
    } finally {
        Pop-Location
    }
}

function Get-PuntoProcesses() {
    return Get-Process | Where-Object { $_.Name -match 'punto|puntoswitcher' }
}

$puntoProcesses = Get-PuntoProcesses

if ($puntoProcesses.Count -gt 0) {
    Write-Host 'Punto Switcher is running:'
    foreach ($p in $puntoProcesses) {
        $path = if ($p.Path) { $p.Path } else { 'unknown path' }
        Write-Host "  $($p.Name) PID $($p.Id) $path"
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
        Write-Host 'ZЯ can conflict with Punto Switcher. Run install.ps1 -StopPunto to close it automatically.'
    }
}

$source = Find-BuiltExecutable

if (-not $source) {
    $downloadUrl = Get-ReleaseDownloadUrl

    if ($downloadUrl) {
        Write-Host "Latest release binary: $downloadUrl"

        if (-not $DryRun -and -not $NoDownload) {
            $downloaded = Join-Path $TempDir 'zya.exe'
            if (Download-File $downloadUrl $downloaded) {
                $source = $downloaded
                Write-Host "Downloaded zya.exe to: $downloaded"
            }
        }
    }
}

if ($DryRun) {
    Write-Host 'Dry run finished. No files were changed.'
    return
}

if (-not $source) {
    if ($Build -or $InstallDependencies) {
        Build-Project
        $source = Find-BuiltExecutable
    }
}

if (-not $source) {
    throw "No zya.exe was found. Run: .\install.ps1 -Build"
}

Write-Host "Using: $source"

if ($DryRun) {
    Write-Host 'Dry run finished. No files were changed.'
    return
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

Copy-Item -LiteralPath $source -Destination $InstalledExe -Force

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

Get-Process |
    Where-Object {
        $_.Path -and (
            $_.Path -like "$InstallDir*" -or
            $_.Path -like "$ConfigDir*"
        )
    } |
    ForEach-Object {
        try {
            Stop-Process -Id $_.Id -Force
        } catch {
        }
    }

try {
    & $InstalledExe install | Out-Null
} catch {
    Write-Host 'Built-in install command failed; continuing with the copied executable.'
}

$finalExe = Join-Path $ConfigDir 'zя.exe'
$runExe = if (Test-Path -LiteralPath $finalExe) { $finalExe } else { $InstalledExe }

Start-Process -FilePath $runExe -ArgumentList 'run', 'headless' -WindowStyle Hidden

Write-Host ''
Write-Host 'zя installed.'
Write-Host "Copy in config folder: $finalExe"
Write-Host "Installer copy: $InstalledExe"
Write-Host 'Autostart: yes'
Write-Host 'Hotkeys: Break, Ctrl+Break, Shift+Break, Ctrl+Alt+P'
Write-Host 'Uninstall: Uninstall.ps1'
