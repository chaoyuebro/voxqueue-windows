param(
    [ValidateSet('Install', 'Status', 'Uninstall')]
    [string]$Action = 'Install',
    [string]$DesktopBinary
)

$ErrorActionPreference = 'Stop'
$startupDirectory = [Environment]::GetFolderPath('Startup')
if ([string]::IsNullOrWhiteSpace($startupDirectory)) {
    throw 'Current user Startup folder is unavailable.'
}
$shortcutPath = Join-Path $startupDirectory 'VoxQueue.lnk'
$previousShortcutPath = Join-Path $startupDirectory 'easyinput.lnk'
$legacyShortcutPath = Join-Path $startupDirectory 'Codex Keyboard.lnk'
$description = 'Start VoxQueue and its local Host at sign-in'
$previousDescription = 'Start easyinput and its local Host at sign-in'
$legacyDescription = 'Start Codex Keyboard and its local Host at sign-in'
$shell = New-Object -ComObject WScript.Shell

if ($Action -eq 'Status') {
    if (-not (Test-Path -LiteralPath $shortcutPath -PathType Leaf)) {
        Write-Output 'startup=absent'
        return
    }
    $shortcut = $shell.CreateShortcut($shortcutPath)
    Write-Output 'startup=present'
    Write-Output "target_present=$(Test-Path -LiteralPath $shortcut.TargetPath -PathType Leaf)"
    return
}

if ($Action -eq 'Uninstall') {
    if (Test-Path -LiteralPath $shortcutPath -PathType Leaf) {
        $shortcut = $shell.CreateShortcut($shortcutPath)
        if ($shortcut.Description -ne $description) {
            throw 'Startup shortcut is not owned by this installer.'
        }
        Remove-Item -LiteralPath $shortcutPath -Force
    }
    Write-Output 'startup=uninstalled'
    return
}

if ([string]::IsNullOrWhiteSpace($DesktopBinary)) {
    throw 'Provide -DesktopBinary with the installed Desktop executable path.'
}
if (-not [IO.Path]::IsPathFullyQualified($DesktopBinary)) {
    throw 'Desktop binary path must be absolute.'
}
# Preserve the caller's path. A sandboxed launcher may remap Resolve-Path to another profile.
$target = [IO.Path]::GetFullPath($DesktopBinary)
if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw 'Desktop binary is missing.'
}
if (Test-Path -LiteralPath $shortcutPath -PathType Leaf) {
    $existing = $shell.CreateShortcut($shortcutPath)
    if ($existing.Description -ne $description) {
        throw 'Startup shortcut is not owned by this installer.'
    }
}
if (Test-Path -LiteralPath $legacyShortcutPath -PathType Leaf) {
    $legacy = $shell.CreateShortcut($legacyShortcutPath)
    if ($legacy.Description -eq $legacyDescription) {
        Remove-Item -LiteralPath $legacyShortcutPath -Force
    }
}
if (Test-Path -LiteralPath $previousShortcutPath -PathType Leaf) {
    $previous = $shell.CreateShortcut($previousShortcutPath)
    if ($previous.Description -eq $previousDescription) {
        Remove-Item -LiteralPath $previousShortcutPath -Force
    }
}
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $target
$shortcut.WorkingDirectory = Split-Path -Path $target -Parent
$shortcut.Description = $description
$shortcut.WindowStyle = 1
$shortcut.Save()
$saved = $shell.CreateShortcut($shortcutPath)
if ($saved.TargetPath -ne $target) {
    throw 'Startup shortcut target did not round-trip.'
}
Write-Output 'startup=installed'
Write-Output 'target_present=True'
