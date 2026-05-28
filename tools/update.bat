@echo off
setlocal

:: Resolve relay root (parent of scripts/)
for %%R in ("%~dp0..") do set "RELAY_ROOT=%%~fR"

cd /d "%RELAY_ROOT%"

echo  Pulling latest Relay from git...
echo.
git pull

echo.
echo  Update complete.
echo.
