# Build the embedded app binary, then build the one-file `z.exe` launcher
# that contains that app binary, application icon, and admin manifest.
$env:PATH += ";$env:USERPROFILE\.cargo\bin"

cargo +stable-x86_64-pc-windows-gnu rustc --release --bin z_single_app -- -o target/release/z_app.exe
Copy-Item target\release\z_app.exe ..\tools\z.bin -Force

cargo +stable-x86_64-pc-windows-gnu rustc --release --bin z_single_launcher -- -o target/release/z.exe

Copy-Item target\release\z.exe ..\release\z.exe -Force
Write-Host "готово: target\release\z.exe и release\z.exe"
