param(
    [switch]$NoDownload
)

$Repo = 'demoonk/Zya'
$ReleaseDir = 'release'
$AppExe = Join-Path $ReleaseDir 'zya_app.exe'
$LauncherExe = Join-Path $ReleaseDir 'zya_launcher.exe'
$Source = 'tools/zya_launcher.rs'

New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null

if (-not $NoDownload -and -not (Test-Path -LiteralPath $AppExe)) {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ User-Agent = 'Zya-Launcher-Builder' }
    $url = $null

    if ($release -and $release.assets) {
        foreach ($asset in $release.assets) {
            if ($asset.name -eq 'zya_app.exe') {
                $url = $asset.browser_download_url
                break
            }
        }
    }

    if (-not $url) {
        $url = "https://github.com/$Repo/releases/download/v0.1.1/zya_app.exe"
    }

    Write-Host "Downloading app binary: $url"
    Invoke-WebRequest -Uri $url -OutFile $AppExe -UserAgent 'Zya-Launcher-Builder' -TimeoutSec 90
}

if (-not (Test-Path -LiteralPath $AppExe)) {
    Write-Host "No embedded app binary found. Put release/zya_app.exe here, or run without -NoDownload."
    exit 1
}

$cargo = Get-Command rustc -ErrorAction SilentlyContinue
if (-not $cargo) {
    $cargoPath = Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe'
    if (Test-Path -LiteralPath $cargoPath) {
        & $cargoPath --target x86_64-pc-windows-gnu -O -o $LauncherExe $Source
        Write-Host "Built launcher: $LauncherExe"
        return
    }
}

if ($cargo) {
    & $cargo.Source --target x86_64-pc-windows-gnu -O -o $LauncherExe $Source
    Write-Host "Built launcher: $LauncherExe"
} else {
    Write-Host 'rustc not found. Install Rust first: winget install Rust.Rust'
    exit 1
}
