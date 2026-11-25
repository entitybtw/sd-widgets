# Build the sd-widgets daemon for Windows and zip it.
#
#   pwsh -File packaging/windows/build.ps1
#
# Produces dist/sdw-daemon-<version>-windows-x86_64.zip containing the daemon,
# the CLI-only build is not needed: the GUI is the default experience.

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$Version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Host "==> building sdw-daemon $Version"
cargo build --release -p sdw-daemon --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$Stage = Join-Path $Root "target\windows-stage\sdw-daemon"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

Copy-Item "target\x86_64-pc-windows-msvc\release\sdw-daemon.exe" $Stage
Copy-Item "README.md" $Stage
Copy-Item "LICENSE" $Stage
Copy-Item "packaging\windows\README-windows.txt" $Stage -ErrorAction SilentlyContinue
Copy-Item "assets\icons\sd-widgets-256.png" $Stage -ErrorAction SilentlyContinue

$Zip = Join-Path $Dist "sdw-daemon-$Version-windows-x86_64.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Zip
Write-Host "==> $Zip"

# Optional: a real installer, only when Inno Setup 6 is on this machine. The
# portable zip above is the release format; the installer is a convenience.
$Iscc = Get-Command iscc -ErrorAction SilentlyContinue
if ($Iscc) {
    Write-Host "==> building the Inno Setup installer"
    & $Iscc.Source "/DAppVersion=$Version" (Join-Path $PSScriptRoot "installer.iss")
    if ($LASTEXITCODE -ne 0) { throw "iscc failed" }
    Write-Host "==> $Dist\sdw-daemon-$Version-windows-x86_64-setup.exe"
} else {
    Write-Host "==> iscc not found; skipped the optional installer"
}
