# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Admin-only checks before installing Linux: BitLocker, TPM, Secure Boot, firmware boot entries.
# Usage: tools\windows\run-elevated.ps1 tools\windows\collect-admin.ps1   (Windows sudo is disabled on this machine)
# Output: hw/windows/admin.txt (summary) and private/windows/admin-full.txt (protector IDs, boot entry GUIDs).
param([string]$Out, [string]$Private)
# $PSScriptRoot is empty in param defaults when run through -Command (run-elevated.ps1)
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'hw\windows' }
if (-not $Private) { $Private = Join-Path $root 'private\windows' }
New-Item -ItemType Directory -Force $Out, $Private | Out-Null
$full = Join-Path $Private 'admin-full.txt'
$sum = Join-Path $Out 'admin.txt'

& {
    '### manage-bde -status'; manage-bde -status
    '### manage-bde -protectors -get C:'; manage-bde -protectors -get C:
    '### Get-Tpm'; Get-Tpm | Format-List
    '### Confirm-SecureBootUEFI'; try { Confirm-SecureBootUEFI } catch { "$_" }
    '### Get-SecureBootPolicy'; try { Get-SecureBootPolicy | Format-List } catch { "$_" }
    '### bcdedit /enum firmware'; bcdedit /enum firmware
    '### reagentc /info'; reagentc /info
    '### powercfg /a'; powercfg /a
    '### HVCI / VBS'; Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard Win32_DeviceGuard | Format-List
} *>&1 | Out-String -Width 4096 | Out-File -Encoding utf8 $full

# Summary without GUIDs/recovery-key IDs
$bl = Get-BitLockerVolume -MountPoint C: -ErrorAction SilentlyContinue
$tpm = Get-Tpm -ErrorAction SilentlyContinue
$sbState = try { Confirm-SecureBootUEFI } catch { "$_" }
$fw = (bcdedit /enum firmware) -match '^description' | ForEach-Object { ($_ -replace '^description\s+', '').Trim() }
[pscustomobject][ordered]@{
    BitLockerC          = if ($bl) { "$($bl.VolumeStatus), protection $($bl.ProtectionStatus), $($bl.EncryptionMethod), $($bl.EncryptionPercentage)%" } else { 'n/a' }
    BitLockerProtectors = if ($bl) { ($bl.KeyProtector.KeyProtectorType -join ', ') } else { 'n/a' }
    TPM                 = if ($tpm) { "present $($tpm.TpmPresent), ready $($tpm.TpmReady), maker $($tpm.ManufacturerIdTxt) $($tpm.ManufacturerVersion)" } else { 'n/a' }
    SecureBoot          = $sbState
    FirmwareBootEntries = ($fw -join ' ; ')
    SleepStates         = ((powercfg /a) -join ' ') -replace '\s+', ' '
} | Format-List | Out-String -Width 4096 | Out-File -Encoding utf8 $sum
Get-Content $sum
