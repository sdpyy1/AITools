@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
cd /d E:\Tools\AITools\BongoCat
cargo build -p bongocat-app --locked --release
if %errorlevel%==0 (
  powershell -NoProfile -Command "$ws=New-Object -ComObject WScript.Shell; $p=Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\BongoCat.lnk'; $l=$ws.CreateShortcut($p); $l.TargetPath='E:\Tools\AITools\BongoCat\target\release\bongocat-app.exe'; $l.WorkingDirectory='E:\Tools\AITools\BongoCat'; $l.Description='BongoCat desktop pet (auto-start, release)'; $l.Save(); Write-Output 'LNK updated to release exe'"
  echo RELEASE_BUILD_AND_LNK_DONE
) else (
  echo RELEASE_BUILD_FAILED
)
