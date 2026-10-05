$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$installerName = 'VoxQueue_0.1.0_x64-setup.exe'
$installer = Join-Path $projectRoot "target\release\bundle\nsis\$installerName"
$images = @(
    @{ Relative = 'firmware/build/bootloader/bootloader.bin'; Sha256 = '001EBD5CAD3E5CCC7E66B13FF9076AF29B510255F6580FE81379F5444BAD4991' },
    @{ Relative = 'firmware/build/partition_table/partition-table.bin'; Sha256 = '7C541B70DCAC8F920C2D11589F06745E1B033FA9B95B8343DE2748BB8312A278' },
    @{ Relative = 'firmware/build/easy_codex_input.bin'; Sha256 = 'AA1FFE4E2D5F8A10576E8273FE45B1E89F75EB91F18670E4528042115E44CE82' }
)
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw 'Build the Windows NSIS installer before packaging.'
}
foreach ($image in $images) {
    $source = Join-Path $projectRoot ($image.Relative -replace '/', '\')
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Missing firmware image: $($image.Relative)"
    }
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $image.Sha256) {
        throw "Firmware image SHA-256 differs from the validated V2 image: $($image.Relative)"
    }
}

$artifactRoot = Join-Path $projectRoot 'artifacts'
New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
$stage = Join-Path $artifactRoot "windows-preview-0.1.0-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
New-Item -ItemType Directory -Path $stage -Force | Out-Null

$sources = @(
    @{ Source = $installer; Relative = $installerName },
    @{ Source = (Join-Path $projectRoot 'docs\windows-setup.md'); Relative = 'windows-setup.md' },
    @{ Source = (Join-Path $projectRoot 'scripts\save-windows-bailian.py'); Relative = 'scripts/save-windows-bailian.py' },
    @{ Source = (Join-Path $projectRoot 'scripts\provision-windows-board.ps1'); Relative = 'scripts/provision-windows-board.ps1' },
    @{ Source = (Join-Path $projectRoot 'scripts\allow-windows-lan-voice.ps1'); Relative = 'scripts/allow-windows-lan-voice.ps1' },
    @{ Source = (Join-Path $projectRoot 'scripts\flash-windows-v2.py'); Relative = 'scripts/flash-windows-v2.py' }
)
foreach ($image in $images) {
    $sources += @{ Source = (Join-Path $projectRoot ($image.Relative -replace '/', '\')); Relative = $image.Relative }
}
$manifest = New-Object System.Collections.Generic.List[string]
foreach ($item in $sources) {
    if (-not (Test-Path -LiteralPath $item.Source -PathType Leaf)) {
        throw "Missing release input: $($item.Relative)"
    }
    $destination = Join-Path $stage ($item.Relative -replace '/', '\')
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $item.Source -Destination $destination -Force
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
    if ($hash -notmatch '^[0-9A-F]{64}$') {
        throw "Failed to hash release input: $($item.Relative)"
    }
    $manifest.Add("$hash  $($item.Relative)")
}
$manifestPath = Join-Path $stage 'SHA256SUMS.txt'
[IO.File]::WriteAllLines($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))

$zip = Join-Path $artifactRoot 'Codex-Keyboard-Windows-Preview-0.1.0.zip'
if (Test-Path -LiteralPath $zip) {
    throw "Release ZIP already exists: $zip"
}
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
Write-Output "release_zip=$zip"
Write-Output "release_sha256=$((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash)"
Write-Output "staging_directory=$stage"
