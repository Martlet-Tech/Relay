@echo off
wt.exe -d "%~dp0." cmd /c python "%~dp0chat.py" %*
if %errorlevel% neq 0 (
    python "%~dp0chat.py" %*
)
