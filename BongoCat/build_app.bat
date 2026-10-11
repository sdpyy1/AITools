@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d E:\Tools\AITools\BongoCat
cargo build -p bongocat-app --locked
