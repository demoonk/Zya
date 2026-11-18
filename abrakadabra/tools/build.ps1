# Build the release binary and produce the distributable name.
# MinGW's ld cannot write an executable whose name contains non-ASCII
# characters, so cargo builds `zya.exe` and we copy it to `zя.exe`.
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
cargo +stable-x86_64-pc-windows-gnu build --release
Copy-Item target\release\zya.exe target\release\zя.exe -Force
Write-Host "готово: target\release\zя.exe"
