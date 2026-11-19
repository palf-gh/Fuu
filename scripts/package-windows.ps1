# Build a release Fuu and pack it as dist\Fuu-<version>-windows-x64.zip.
# Unsigned: signing arrives later (see docs/plan.md, M5).
#
#   pwsh scripts\package-windows.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "Fuu-$version-windows-x64"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name

cargo build -p fuu-app --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\fuu.exe (Join-Path $stage 'Fuu.exe')
Copy-Item LICENSE, README.md $stage

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
"$hash  $name.zip" | Set-Content -Encoding ascii (Join-Path $dist "$name.zip.sha256")
Write-Host "$zip"
Write-Host "sha256 $hash"
