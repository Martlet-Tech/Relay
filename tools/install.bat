@echo off
setlocal enabledelayedexpansion

echo.
echo  === Relay Install (Rust) ===
echo.

:: Resolve relay root
for %%R in ("%~dp0..") do set "RELAY_ROOT=%%~fR"
set "BIN_DIR=%USERPROFILE%\.relay\bin"
set OK=1

:: ── 1. Build release binary ──
echo  [..] Building relay.exe (cargo build --release)...
cd /d "%RELAY_ROOT%"
call cargo build --release 2>&1
if %errorlevel% equ 0 (
    echo  [✓] Build successful
) else (
    echo  [✗] Build failed
    set OK=0
)

:: ── 2. Create bin directory ──
if not exist "%BIN_DIR%" mkdir "%BIN_DIR%" 2>nul
if exist "%BIN_DIR%" (
    echo  [✓] Directory: %BIN_DIR%
) else (
    echo  [✗] Failed to create directory
    set OK=0
)

:: ── 3. Copy binary ──
if !OK!==1 (
    copy /Y "%RELAY_ROOT%\target\release\relay.exe" "%BIN_DIR%\relay.exe" >nul
    if exist "%BIN_DIR%\relay.exe" (
        echo  [✓] Binary: %BIN_DIR%\relay.exe
    ) else (
        echo  [✗] Failed to copy binary
        set OK=0
    )
)

:: ── 4. Add to user PATH ──
if !OK!==1 (
    powershell -NoProfile -Command "$bin='%BIN_DIR%';$p=[Environment]::GetEnvironmentVariable('PATH','User');if($p){$e=$p.Split(';');if($e -notcontains $bin){[Environment]::SetEnvironmentVariable('PATH',$p.TrimEnd(';')+';'+$bin,'User');exit 0}else{exit 1}}else{[Environment]::SetEnvironmentVariable('PATH',$bin,'User');exit 0}"
    if !errorlevel! equ 0 (
        echo  [✓] PATH updated
    ) else if !errorlevel! equ 1 (
        echo  [•] Already in PATH, skipped
    ) else (
        echo  [✗] Failed to update PATH
        set OK=0
    )
)

:: ── Done ──
echo.
if !OK!==1 (
    echo  ✓ Installation successful.
    echo.
    echo  Now type "relay" in any terminal to start.
    echo  To uninstall later, run: tools\uninstall.bat
) else (
    echo  ✗ Installation failed. Check messages above.
)
echo.
pause
