@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

title kynxShare — Build & Install
color 0A

echo.
echo  ========================================
echo   kynxShare  -  Build and Install
echo  ========================================
echo.

where rustc >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Rust not found. Install from https://rustup.rs/
  goto :fail
)

where npm >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Node.js / npm not found. Install from https://nodejs.org/
  goto :fail
)

echo [1/3] Installing frontend dependencies...
pushd "apps\desktop"
call npm install
if errorlevel 1 (
  echo [ERROR] npm install failed.
  popd
  goto :fail
)

echo.
echo [2/3] Building release installer (this can take several minutes)...
call npm run tauri build
if errorlevel 1 (
  echo [ERROR] tauri build failed.
  popd
  goto :fail
)
popd

echo.
echo [3/3] Locating installer...

set "BUNDLE=apps\desktop\src-tauri\target\release\bundle"
set "INSTALLER="

REM Prefer NSIS .exe, then MSI
for %%F in ("%BUNDLE%\nsis\*.exe") do (
  set "INSTALLER=%%~fF"
  goto :found
)
for %%F in ("%BUNDLE%\msi\*.msi") do (
  set "INSTALLER=%%~fF"
  goto :found
)

echo [ERROR] No installer found under:
echo   %BUNDLE%\nsis\  or  %BUNDLE%\msi\
echo Build may have succeeded without bundling. Check the Tauri build log.
goto :fail

:found
echo Found: !INSTALLER!
echo.
echo Starting installer...
echo.

start "" /wait "!INSTALLER!"
if errorlevel 1 (
  echo.
  echo Installer exited with an error code. You can still run it manually:
  echo   !INSTALLER!
  goto :fail
)

echo.
echo  ========================================
echo   Done. kynxShare should be installed.
echo  ========================================
echo.
pause
exit /b 0

:fail
echo.
echo Build/install aborted.
pause
exit /b 1
