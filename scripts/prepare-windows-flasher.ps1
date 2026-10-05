param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$voxRoot = Split-Path -Parent $PSScriptRoot
$voxResources = Join-Path $voxRoot 'app\desktop\src-tauri\resources'
$voxFlashSource = Get-Content -LiteralPath (Join-Path $voxRoot 'app\desktop\src-tauri\src\firmware_flash.rs') -Raw
New-Item -ItemType Directory -Path $voxResources -Force | Out-Null
$voxImageSpecs = @(
    @('bootloader-20261005.bin', 'current-bootloader.bin', 'BOOTLOADER_SHA'),
    @('partition-table-20261005.bin', 'current-partition-table.bin', 'PARTITION_SHA'),
    @('keyboard-volume-20261006.bin', 'current-firmware.bin', 'FIRMWARE_SHA')
)
foreach ($voxImageSpec in $voxImageSpecs) {
    $voxFirmware = Join-Path $voxRoot ('firmware\releases\' + $voxImageSpec[0])
    $voxExpectedFirmware = [regex]::Match($voxFlashSource, ($voxImageSpec[2] + ': &str = "([0-9a-f]{64})"')).Groups[1].Value
    if (-not (Test-Path -LiteralPath $voxFirmware) -or
        (Get-FileHash -LiteralPath $voxFirmware -Algorithm SHA256).Hash.ToLowerInvariant() -ne $voxExpectedFirmware) {
        throw ('Reviewed restore image is missing or its SHA-256 does not match: ' + $voxImageSpec[0])
    }
    Copy-Item -LiteralPath $voxFirmware -Destination (Join-Path $voxResources $voxImageSpec[1]) -Force
    Write-Output ($voxImageSpec[1] + '=' + $voxExpectedFirmware)
}
$voxArchive = Join-Path ([IO.Path]::GetTempPath()) 'esptool-v4.12.0-windows-amd64.zip'
$voxToolDirectory = Join-Path $voxResources 'esptool'
if (-not (Test-Path -LiteralPath $voxArchive)) {
    if ($Offline) { throw 'esptool cache missing. Run prepare-windows-flasher.ps1 once without -Offline.' }
    Invoke-WebRequest 'https://github.com/espressif/esptool/releases/download/v4.12.0/esptool-v4.12.0-windows-amd64.zip' -OutFile $voxArchive
}
if ((Get-FileHash -LiteralPath $voxArchive -Algorithm SHA256).Hash -ne '42FDDC5E6A05716868AD77FB43ACBF53BE041F97ABED87FF850DF1DC88140889') {
    throw 'Official esptool archive SHA-256 mismatch.'
}
New-Item -ItemType Directory -Path $voxToolDirectory -Force | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
$voxZip = [IO.Compression.ZipFile]::OpenRead($voxArchive)
try {
    foreach ($voxFile in @('esptool.exe', 'LICENSE', 'README.md')) {
        $voxEntry = $voxZip.GetEntry('esptool-windows-amd64/' + $voxFile)
        if ($null -eq $voxEntry) { throw 'esptool archive is incomplete.' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($voxEntry, (Join-Path $voxToolDirectory $voxFile), $true)
    }
} finally { $voxZip.Dispose() }
Write-Output 'flasher=esptool-4.12.0-bundled'
