# Build a release Fuu and pack it into dist\:
#   Fuu-<version>-windows-x64-setup.exe  installer (Inno Setup 6, when installed)
#   Fuu-<version>-windows-x64.zip        portable copy
# each with a .sha256 file. Unsigned: signing arrives later (docs/plan.md, M5).
#
#   pwsh scripts\package-windows.ps1            # installer + zip
#   pwsh scripts\package-windows.ps1 -NoInstaller
param([switch]$NoInstaller)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "Fuu-$version-windows-x64"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name

function Write-Checksum([string]$file) {
    $hash = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant()
    "$hash  $(Split-Path $file -Leaf)" | Set-Content -Encoding ascii "$file.sha256"
    Write-Host "$file"
    Write-Host "  sha256 $hash"
}

cargo build -p fuu-app --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\fuu.exe (Join-Path $stage 'Fuu.exe')
Copy-Item LICENSE, README.md $stage

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Checksum $zip

if (-not $NoInstaller) {
    $iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
            Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found; install it or pass -NoInstaller" }
    & $iscc /Q "/DAppVersion=$version" "/DSourceDir=$stage" "/DOutputDir=$dist" (Join-Path $root 'packaging\windows\fuu.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
    Write-Checksum (Join-Path $dist "$name-setup.exe")
}
