@echo off
setlocal enabledelayedexpansion

echo.
echo  === Relay Uninstall ===
echo.

set "BIN_DIR=%USERPROFILE%\.relay\bin"
set OK=1

:: ── 1. Remove launcher ──
if exist "%BIN_DIR%\relay.cmd" (
    del "%BIN_DIR%\relay.cmd"
    if exist "%BIN_DIR%\relay.cmd" (
        echo  [✗] Failed to delete launcher
        set OK=0
    ) else (
        echo  [✓] Launcher removed
    )
) else (
    echo  [•] No launcher found, skipped
)

:: ── 2. Remove from PATH ──
powershell -NoProfile -Command "$bin='%BIN_DIR%';$p=[Environment]::GetEnvironmentVariable('PATH','User');if($p){$e=($p.Split(';')|Where-Object{$_ -ne $bin}) -join ';';if($e -ne $p){[Environment]::SetEnvironmentVariable('PATH',$e,'User');exit 0}else{exit 1}}else{exit 2}"
if !errorlevel! equ 0 (
    echo  [✓] PATH cleaned
) else if !errorlevel! equ 1 (
    echo  [•] Was not in PATH, skipped
) else (
    echo  [•] PATH is empty, skipped
)

:: ── 3. Remove empty directory ──
rmdir "%BIN_DIR%" 2>nul
if not exist "%BIN_DIR%" (
    echo  [✓] Directory removed
)

:: ── Done ──
echo.
echo  ✓ Uninstall complete.
echo.
pause
