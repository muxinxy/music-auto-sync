@echo off
setlocal
call "D:\Dev\VSBuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
mise exec -- npm run tauri build
