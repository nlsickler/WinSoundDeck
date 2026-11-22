<#
.SYNOPSIS
    Builds WinSoundDeck Portable (.exe) and NSIS Installer targets into .\dist\.
#>
[CmdletBinding()]
param(
    [switch]$PortableOnly,
    [switch]$InstallerOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    New-Item -ItemType Directory -Force dist | Out-Null

    if (-not $InstallerOnly) {
        Write-Host "==> Building Portable executable (release, --features portable)..."
        cargo build --release -p audio-ui --features portable --offline
        Copy-Item "target\release\audio-ui.exe" "dist\WinSoundDeck-portable.exe" -Force
        Write-Host "Portable executable: $(Resolve-Path 'dist\WinSoundDeck-portable.exe')"
    }

    if (-not $PortableOnly) {
        if (-not (Get-Command cargo-tauri -ErrorAction SilentlyContinue)) {
            $hasSubcmd = (cargo --list | Select-String -Pattern '^\s+tauri\b')
            if (-not $hasSubcmd) {
                Write-Warning "tauri-cli is not installed. Install with: cargo install tauri-cli --version '^2'"
                exit 0
            }
        }
        Write-Host "==> Building audio-daemon sidecar (release)..."
        cargo build --release -p audio-daemon --offline
        New-Item -ItemType Directory -Force "crates\audio-ui\binaries" | Out-Null
        Copy-Item "target\release\audio-daemon.exe" "crates\audio-ui\binaries\audio-daemon-x86_64-pc-windows-msvc.exe" -Force

        Write-Host "==> Building NSIS installer via cargo tauri build..."
        cargo tauri build --config crates/audio-ui/tauri.installer.conf.json
        Get-ChildItem "target\release\bundle\nsis\*.exe" -ErrorAction SilentlyContinue | ForEach-Object {
            Copy-Item $_.FullName "dist\" -Force
            Write-Host "Installer: $(Join-Path (Resolve-Path 'dist') $_.Name)"
        }
    }
} finally {
    Pop-Location
}
