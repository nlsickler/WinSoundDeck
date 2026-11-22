<#
.SYNOPSIS
    Installs build-time developer prerequisites for WinSoundDeck packaging.
.DESCRIPTION
    Verifies Rust toolchain availability and installs tauri-cli (^2) if missing,
    which is required to build the NSIS Windows installer via scripts\build-release.ps1.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

foreach ($tool in @('cargo', 'rustc')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required tool '$tool' not found on PATH. Install Rust via https://rustup.rs/ first."
    }
}

$hasTauri = (Get-Command cargo-tauri -ErrorAction SilentlyContinue) -or
    (cargo --list | Select-String -Pattern '^\s+tauri\b')

if ($hasTauri) {
    $ver = (cargo tauri --version 2>&1 | Out-String).Trim()
    Write-Host "tauri-cli already installed: $ver"
} else {
    Write-Host "==> Installing tauri-cli (^2)..."
    cargo install tauri-cli --version "^2" --locked
}

Write-Host "Setup complete. Note: NSIS is auto-downloaded by Tauri on the first 'cargo tauri build' run."
