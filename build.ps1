param(
    [switch]$Debug
)

$ErrorActionPreference = 'Stop'

$root = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$project = Join-Path $root 'abrakadabra'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo is not installed. Install Rust first: https://rustup.rs"
}

Push-Location $project
try {
    if ($Debug) {
        cargo build
    } else {
        cargo build --release
    }
} catch {
    Write-Host ''
    Write-Host 'Build failed.'
    Write-Host 'If the message says "linker link.exe not found", install Visual Studio Build Tools with the "C++" workload.'
    Write-Host 'Example: winget install Microsoft.VisualStudio.2022.BuildTools'
    throw
} finally {
    Pop-Location
}

Write-Host ''
Write-Host 'Build completed. Use install.ps1 to install the compiled app.'
