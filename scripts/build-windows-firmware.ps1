param(
    [string]$IdfPath = 'C:\esp\v5.5.5\esp-idf',
    [string]$ToolsRoot = 'C:\Espressif'
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$firmwarePath = Join-Path $projectRoot 'firmware'
$python = Join-Path $ToolsRoot 'tools\python\v5.5.5\venv\Scripts\python.exe'
$idf = Join-Path $IdfPath 'tools\idf.py'
$toolBins = @(
    (Join-Path $ToolsRoot 'tools\xtensa-esp-elf\esp-14.2.0_20260121\xtensa-esp-elf\bin'),
    (Join-Path $ToolsRoot 'tools\ninja\1.12.1'),
    (Join-Path $ToolsRoot 'tools\cmake\3.30.2\bin'),
    (Join-Path $ToolsRoot 'tools\python\v5.5.5\venv\Scripts')
)
foreach ($path in @($IdfPath, $python, $idf) + $toolBins) {
    if (-not (Test-Path -LiteralPath $path)) { throw "Missing ESP-IDF dependency: $path" }
}

$env:IDF_PATH = $IdfPath
# ESP-IDF subprocesses must read UTF-8 configs when the checkout path contains Chinese.
$env:PYTHONUTF8 = '1'
$env:IDF_TOOLS_PATH = $ToolsRoot
$env:IDF_PYTHON_ENV_PATH = Split-Path -Parent (Split-Path -Parent $python)
$env:ESP_ROM_ELF_DIR = Join-Path $ToolsRoot 'tools\esp-rom-elfs\20241011'
$env:PATH = ($toolBins -join ';') + ';' + $env:PATH

Push-Location $firmwarePath
try {
    & $python $idf build
    if ($LASTEXITCODE -ne 0) { throw "ESP-IDF build failed with exit code $LASTEXITCODE" }
    $images = @(
        'build\bootloader\bootloader.bin',
        'build\partition_table\partition-table.bin',
        'build\easy_codex_input.bin'
    )
    foreach ($image in $images) {
        $digest = Get-FileHash -LiteralPath $image -Algorithm SHA256
        Write-Output "$($digest.Hash) $image"
    }
    Write-Host 'Build complete. No device was reset or flashed.'
} finally {
    Pop-Location
}
