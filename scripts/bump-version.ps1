<#
.SYNOPSIS
    Bumps the workspace SemVer version, commits, and creates an annotated Git tag.
.EXAMPLE
    .\scripts\bump-version.ps1 patch
    .\scripts\bump-version.ps1 minor
    .\scripts\bump-version.ps1 1.0.0
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Target,
    [switch]$NoCommit,
    [switch]$AllowDirty
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    if (-not $AllowDirty -and -not $NoCommit) {
        $dirty = git status --porcelain
        if ($dirty) {
            throw "Working tree is not clean. Commit or stash changes before cutting a release (or pass -AllowDirty)."
        }
    }

    $cargoPath = Join-Path $root "Cargo.toml"
    $tauriPath = Join-Path $root "crates\audio-ui\tauri.conf.json"

    $cargoRaw = Get-Content -Path $cargoPath -Raw
    if ($cargoRaw -notmatch '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"') {
        throw "Could not find [workspace.package] version in Cargo.toml"
    }
    $maj = [int]$Matches[1]
    $min = [int]$Matches[2]
    $pat = [int]$Matches[3]
    $current = "$maj.$min.$pat"

    switch ($Target.ToLowerInvariant()) {
        'patch' { $next = "$maj.$min.$($pat + 1)" }
        'minor' { $next = "$maj.$($min + 1).0" }
        'major' { $next = "$($maj + 1).0.0" }
        default {
            if ($Target -notmatch '^\d+\.\d+\.\d+$') {
                throw "Invalid version target '$Target'. Use 'patch', 'minor', 'major', or 'X.Y.Z'."
            }
            $next = $Target
        }
    }

    Write-Host "Bumping version: $current -> $next"

    $cargoUpdated = $cargoRaw -replace '(?m)^(version\s*=\s*")\d+\.\d+\.\d+(")', "`${1}$next`${2}"
    Set-Content -Path $cargoPath -Value $cargoUpdated -Encoding UTF8 -NoNewline

    $tauriRaw = Get-Content -Path $tauriPath -Raw
    $tauriUpdated = $tauriRaw -replace '(?m)("version"\s*:\s*")\d+\.\d+\.\d+(")', "`${1}$next`${2}"
    Set-Content -Path $tauriPath -Value $tauriUpdated -Encoding UTF8 -NoNewline

    cargo metadata --offline --format-version 1 | Out-Null

    if (-not $NoCommit) {
        $tag = "v$next"
        git add Cargo.toml Cargo.lock crates/audio-ui/tauri.conf.json
        git commit -m "chore(release): $tag"
        git tag -a $tag -m "Release $tag"
        Write-Host "Created commit and tag '$tag'. Push with: git push origin main --follow-tags"
    }
} finally {
    Pop-Location
}
