@echo off
if exist "%~dp0install.ps1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
) else (
    echo install.ps1 was not next to this batch file.
)
pause
