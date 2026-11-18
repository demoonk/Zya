@echo off
if exist "%~dp0Uninstall.ps1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Uninstall.ps1" %*
) else (
    echo Uninstall.ps1 was not next to this batch file.
)
pause
