@echo off
setlocal enabledelayedexpansion

echo.
echo  === Relay Install ===
echo.

:: Resolve relay root (parent of scripts/)
for %%R in ("%~dp0..") do set "RELAY_ROOT=%%~fR"
set "BIN_DIR=%USERPROFILE%\.relay\bin"
set OK=1

:: ── 1. Create bin directory ──
if not exist "%BIN_DIR%" mkdir "%BIN_DIR%" 2>nul
if exist "%BIN_DIR%" (
    echo  [✓] Directory: %BIN_DIR%
) else (
    echo  [✗] Failed to create directory
    set OK=0
)

:: ── 2. Create relay.cmd launcher ──
if !OK!==1 (
    > "%BIN_DIR%\relay.cmd" (
        echo @echo off
        echo python "%RELAY_ROOT%\chat.py" %%*
    )
    if exist "%BIN_DIR%\relay.cmd" (
        echo  [✓] Launcher: %BIN_DIR%\relay.cmd
        echo      Calls: %RELAY_ROOT%\chat.py
    ) else (
        echo  [✗] Failed to create launcher
        set OK=0
    )
)

:: ── 3. Add to user PATH ──
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
    echo  If not found, restart your terminal or run: refreshenv
) else (
    echo  ✗ Installation failed. Check messages above.
)
echo.
pause
