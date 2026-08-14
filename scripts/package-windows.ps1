# 组装 dsh-launcher Windows 便携包(CI 用)
# 用法:powershell -File scripts/package-windows.ps1 -Version 0.2.0 -OutDir build -Exe build\dsh-launcher.exe
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$OutDir,
  [Parameter(Mandatory = $true)][string]$Exe
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Pkg = Join-Path $OutDir 'dsh-launcher-windows-x64'

if (Test-Path $Pkg) { Remove-Item -Recurse -Force $Pkg }
New-Item -ItemType Directory -Force -Path (Join-Path $Pkg "apps\$Version") | Out-Null

Copy-Item $Exe (Join-Path $Pkg 'dsh-launcher.exe')
Copy-Item (Join-Path $Root 'assets\icon.ico') (Join-Path $Pkg 'app.ico')
Set-Content -Path (Join-Path $Pkg 'launcher.json') -Value "{`"current`":`"$Version`"}" -Encoding utf8

foreach ($d in @('src', 'public', 'bin', 'scripts')) {
  Copy-Item -Recurse (Join-Path $Root $d) (Join-Path $Pkg "apps\$Version\$d")
}
foreach ($f in @('LICENSE', 'README.md', 'package.json')) {
  Copy-Item (Join-Path $Root $f) (Join-Path $Pkg "apps\$Version\$f")
}

Write-Host "已生成 $Pkg"
