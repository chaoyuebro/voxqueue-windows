param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$voxRoot = Split-Path -Parent $PSScriptRoot
$voxResources = Join-Path $voxRoot 'app\desktop\src-tauri\resources'
$voxFirmware = Join-Path $voxRoot 'firmware\releases\desktop-presence-20261005.bin'
$voxFlashSource = Get-Content -LiteralPath (Join-Path $voxRoot 'app\desktop\src-tauri\src\firmware_flash.rs') -Raw
$voxExpectedFirmware = [regex]::Match($voxFlashSource, 'FIRMWARE_SHA: &str = "([0-9a-f]{64})"').Groups[1].Value
if (-not (Test-Path -LiteralPath $voxFirmware) -or
    (Get-FileHash -LiteralPath $voxFirmware -Algorithm SHA256).Hash.ToLowerInvariant() -ne $voxExpectedFirmware) {
    throw 'The reviewed firmware release is missing or its SHA-256 does not match firmware_flash.rs.'
}
New-Item -ItemType Directory -Path $voxResources -Force | Out-Null
Copy-Item -LiteralPath $voxFirmware -Destination (Join-Path $voxResources 'current-firmware.bin') -Force
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
Write-Output "firmware=$voxExpectedFirmware"
Write-Output 'flasher=esptool-4.12.0-bundled'
