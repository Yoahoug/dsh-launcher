@echo off
rem dsh-launcher · Windows 双击入口(git 检出 / 便携包均可)
rem 行为:起服务(http://127.0.0.1:3090/)→ 自动打开浏览器亮色控制台;已运行则只召回(单实例)。
setlocal
cd /d "%~dp0"
set "URL=http://127.0.0.1:3090"

where node >nul 2>nul
if errorlevel 1 (
  echo [dsh-launcher] 未找到 Node.js,请先安装(需要 ^22.19 或 24+)
  pause
  exit /b 1
)

curl -fsS -m 1 %URL%/api/health >nul 2>nul
if not errorlevel 1 (
  start "" %URL%
  exit /b 0
)

mkdir "%USERPROFILE%\.local\state\dsh-launcher\logs" 2>nul
start "dsh-launcher" /min cmd /c "node src\server.mjs >> "%USERPROFILE%\.local\state\dsh-launcher\logs\launcher.out.log" 2>&1"

for /l %%i in (1,1,30) do (
  timeout /t 1 /nobreak >nul
  curl -fsS -m 1 %URL%/api/health >nul 2>nul
  if not errorlevel 1 (
    start "" %URL%
    exit /b 0
  )
)

echo [dsh-launcher] 服务启动超时,日志见 %USERPROFILE%\.local\state\dsh-launcher\logs\
pause
endlocal
