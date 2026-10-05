param([switch]$Offline)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$toolchainRoot = Join-Path (Split-Path $projectRoot -Parent) '.work\codex-toolchain'
if (Test-Path -LiteralPath (Join-Path $toolchainRoot 'cargo\bin\cargo.exe')) {
    $env:CARGO_HOME = Join-Path $toolchainRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolchainRoot 'rustup'
    $env:PATH = "$(Join-Path $env:CARGO_HOME 'bin');$env:PATH"
}

Push-Location $projectRoot
try {
    $cargoArgs = @('build', '-p', 'easy-codex-host', '--bin', 'easy-codex-host', '--release', '--locked')
    if ($Offline) { $cargoArgs += '--offline' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Host release build failed.' }
    $resourceDirectory = Join-Path $projectRoot 'app\desktop\src-tauri\resources'
    New-Item -ItemType Directory -Path $resourceDirectory -Force | Out-Null
    $hostBinary = Join-Path $projectRoot 'target\release\easy-codex-host.exe'
    Copy-Item -LiteralPath $hostBinary -Destination (Join-Path $resourceDirectory 'easy-codex-host.exe') -Force
    $tauri = Join-Path $projectRoot 'app\desktop\node_modules\.bin\tauri.cmd'
    if (-not (Test-Path -LiteralPath $tauri -PathType Leaf)) {
        throw 'Tauri CLI is missing; run pnpm install --frozen-lockfile first.'
    }
    Push-Location (Join-Path $projectRoot 'app\desktop')
    try {
        & $tauri build --bundles nsis
        if ($LASTEXITCODE -ne 0) { throw 'Windows NSIS build failed.' }
    } finally {
        Pop-Location
    }
    Get-FileHash -LiteralPath (Join-Path $projectRoot 'target\release\bundle\nsis\VoxQueue_0.1.0_x64-setup.exe') -Algorithm SHA256 |
        Select-Object -ExpandProperty Hash
} finally {
    Pop-Location
}
