# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Collect hardware information on Windows for the Linux port.
# Runs without admin. Output: hw/windows/ (model-level; reviewed copies are published under docs/reference/) and private/windows/
# (serials, MACs; gitignored).
# ACPI tables: dump-acpi.ps1.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\collect-hwinfo.ps1
param([string]$Out, [string]$Private)
# $PSScriptRoot is empty in param defaults when run through -Command (run-elevated.ps1)
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'hw\windows' }
if (-not $Private) { $Private = Join-Path $root 'private\windows' }
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out, $Private | Out-Null
$Out = (Resolve-Path $Out).Path
$Private = (Resolve-Path $Private).Path

function Save-Text($obj, $path) { $obj | Out-String -Width 4096 | Out-File -Encoding utf8 $path }
function Save-Csv($obj, $path) { $obj | Export-Csv -NoTypeInformation -Encoding UTF8 $path }

# System identity
$cs = Get-CimInstance Win32_ComputerSystem
$bios = Get-CimInstance Win32_BIOS
$os = Get-CimInstance Win32_OperatingSystem
$sb = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' -ErrorAction SilentlyContinue).UEFISecureBootEnabled
$blShell = (New-Object -ComObject Shell.Application).NameSpace('C:').Self.ExtendedProperty('System.Volume.BitLockerProtection')
# Shell BitLockerProtection value; the exact meaning needs `manage-bde -status` (admin), see tools/windows/collect-admin.ps1
$sys = [ordered]@{
    CollectedAt     = (Get-Date -Format 'yyyy-MM-dd HH:mm K')
    Manufacturer    = $cs.Manufacturer
    Model           = $cs.Model
    SKU             = $cs.SystemSKUNumber
    Family          = $cs.SystemFamily
    MemoryGiB       = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
    UEFIVersion     = $bios.SMBIOSBIOSVersion
    UEFIDate        = $bios.ReleaseDate.ToString('yyyy-MM-dd')
    OS              = "$($os.Caption) $($os.Version) build $($os.BuildNumber)"
    SecureBoot      = $sb
    BitLockerC      = $blShell
    FirmwareType    = $env:firmware_type
}
Save-Text ([pscustomobject]$sys | Format-List) "$Out\system.txt"
Save-Text (Get-CimInstance Win32_Processor | Select Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, L2CacheSize, L3CacheSize, VirtualizationFirmwareEnabled | Format-List) "$Out\cpu.txt"
Save-Text (Get-CimInstance Win32_PhysicalMemory | Select Manufacturer, PartNumber, Capacity, Speed, ConfiguredClockSpeed, SMBIOSMemoryType, FormFactor | Format-List) "$Out\memory.txt"
Save-Text ($bios | Select * -ExcludeProperty Cim*, PSComputerName | Format-List) "$Private\bios-full.txt"
Save-Text ($cs | Select * -ExcludeProperty Cim*, PSComputerName | Format-List) "$Private\computersystem-full.txt"

# PnP devices: hardware/compatible IDs (committed), instance IDs (may hold serials, private)
# Win32_PnPEntity/Win32_PnPSignedDriver in two bulk queries; per-device Get-PnpDeviceProperty takes minutes
$drv = @{}
Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceID } | ForEach-Object { $drv[$_.DeviceID] = $_ }
$devs = Get-CimInstance Win32_PnPEntity | Where-Object { $_.Present } | Sort PNPClass, Name
$rows = foreach ($d in $devs) {
    $sd = $drv[$d.DeviceID]
    [pscustomobject]@{
        Class        = $d.PNPClass
        FriendlyName = $d.Name
        Status       = $d.Status
        Manufacturer = $d.Manufacturer
        Service      = $d.Service
        DriverVer    = $sd.DriverVersion
        Inf          = $sd.InfName
        # Bluetooth LE hardware IDs carry the peer address (BTHLE\Dev_<12 hex>)
        HardwareIds  = (($d.HardwareID -join ' | ') -replace '(?i)(Dev_)[0-9a-f]{12}', '$1<addr>')
        CompatIds    = ($d.CompatibleID -join ' | ')
        InstanceId   = $d.DeviceID
    }
}
Save-Csv ($rows | Select * -ExcludeProperty InstanceId) "$Out\pnp-devices.csv"
Save-Csv $rows "$Private\pnp-devices-full.csv"

# ACPI-enumerated devices only, the short list for mapping to Linux drivers
# HID is the "*XXXX" form, which is the _HID Linux matches on; Sub is the Surface subsystem (MSHWnnnn) if any
$acpi = $rows | Where-Object { $_.HardwareIds -match '(^|\| )ACPI\\' } | ForEach-Object {
    $ids = $_.HardwareIds -split ' \| '
    $hid = ($ids | Where-Object { $_ -like '`**' } | Select -First 1) -replace '^\*', ''
    if (-not $hid) { $hid = ($ids[0] -replace '^ACPI\\', '') }
    $sub = if ($ids[0] -match 'SUBSYS_(\w+)') { $Matches[1] } else { '' }
    [pscustomobject]@{ HID = $hid; Sub = $sub; Class = $_.Class; Name = $_.FriendlyName; Service = $_.Service; Status = $_.Status }
} | Group-Object HID, Sub, Name | ForEach-Object {
    $f = $_.Group[0]
    [pscustomobject]@{ HID = $f.HID; Sub = $f.Sub; N = $_.Count; Class = $f.Class; Name = $f.Name; Service = $f.Service; Status = $f.Status }
} | Sort HID
Save-Text ($acpi | Format-Table -AutoSize) "$Out\acpi-devices.txt"

# PCI and USB, short form
$pci = $rows | Where-Object { $_.HardwareIds -match '^PCI\\' } | ForEach-Object {
    [pscustomobject]@{ Id = ($_.HardwareIds -split ' \| ')[0]; Class = $_.Class; Name = $_.FriendlyName; Service = $_.Service }
} | Sort Id
Save-Text ($pci | Format-Table -AutoSize) "$Out\pci-devices.txt"
$usb = $rows | Where-Object { $_.HardwareIds -match '^USB\\VID' } | ForEach-Object {
    [pscustomobject]@{ Id = ($_.HardwareIds -split ' \| ')[0]; Class = $_.Class; Name = $_.FriendlyName; Service = $_.Service }
} | Sort Id
Save-Text ($usb | Format-Table -AutoSize) "$Out\usb-devices.txt"

# Firmware components (UEFI capsule targets: SAM, ME, touch, ...)
$fw = $rows | Where-Object { $_.Class -eq 'Firmware' } | Select FriendlyName, DriverVer, HardwareIds
Save-Text ($fw | Format-Table -AutoSize -Wrap) "$Out\firmware-components.txt"

# Storage layout (for dual boot planning); disk serials go to private
Save-Text (Get-Disk | Select Number, FriendlyName, Model, BusType, PartitionStyle, @{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}}, LogicalSectorSize, PhysicalSectorSize | Format-List) "$Out\disks.txt"
Save-Text (Get-Disk | Select Number, FriendlyName, SerialNumber, UniqueId | Format-List) "$Private\disks-serial.txt"
Save-Text (Get-Partition | Select DiskNumber, PartitionNumber, DriveLetter, GptType, Type, @{n='OffsetMiB';e={[math]::Round($_.Offset/1MB)}}, @{n='SizeMiB';e={[math]::Round($_.Size/1MB)}}, IsSystem, IsBoot, IsHidden | Format-Table -AutoSize) "$Out\partitions.txt"
Save-Text (Get-Volume | Select DriveLetter, FileSystemLabel, FileSystem, @{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}}, @{n='FreeGB';e={[math]::Round($_.SizeRemaining/1GB,1)}}, HealthStatus | Format-Table -AutoSize) "$Out\volumes.txt"

# Display, graphics, network, battery
Save-Text (Get-CimInstance Win32_VideoController | Select Name, DriverVersion, VideoProcessor, AdapterRAM, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate, MaxRefreshRate | Format-List) "$Out\display.txt"
try {
    # Only the internal panel: external monitors' EDIDs carry their serial numbers and change with what is plugged in
    $internal = 'DISPLAY\\LGD0719\\'
    $mon = Get-CimInstance -Namespace root\wmi WmiMonitorID | Where-Object { $_.InstanceName -match "^$internal" } | ForEach-Object {
        [pscustomobject]@{
            Maker   = -join ($_.ManufacturerName | Where-Object { $_ } | ForEach-Object { [char]$_ })
            Product = -join ($_.ProductCodeID | Where-Object { $_ } | ForEach-Object { [char]$_ })
            Name    = -join ($_.UserFriendlyName | Where-Object { $_ } | ForEach-Object { [char]$_ })
            Year    = $_.YearOfManufacture
        }
    }
    Save-Text ($mon | Format-List) "$Out\monitor.txt"
    Get-CimInstance -Namespace root\wmi WmiMonitorDescriptorMethods -ErrorAction Stop | Where-Object { $_.InstanceName -match "^$internal" } | ForEach-Object {
        $r = Invoke-CimMethod -InputObject $_ -MethodName WmiGetMonitorRawEEdidV1Block -Arguments @{ BlockId = [byte]0 }
        if ($r.BlockContent) { [IO.File]::WriteAllBytes("$Out\edid-block0.bin", [byte[]]$r.BlockContent) }
    }
} catch { "monitor: $_" | Out-File -Append -Encoding utf8 "$Out\errors.txt" }
Save-Text (Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Where-Object { $_.HardwareInterface } | Select Name, InterfaceDescription, DriverVersion, DriverDate, DriverProvider, Status, LinkSpeed | Format-List) "$Out\network.txt"
Save-Text (Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Select Name, InterfaceDescription, MacAddress | Format-Table -AutoSize) "$Private\network-mac.txt"
Save-Text (Get-CimInstance Win32_Battery | Select Name, Chemistry, DesignVoltage, EstimatedChargeRemaining, BatteryStatus | Format-List) "$Out\battery.txt"
powercfg /batteryreport /xml /output "$Private\battery-report.xml" | Out-Null
try {
    [xml]$br = Get-Content "$Private\battery-report.xml"
    $b = $br.BatteryReport.Batteries.Battery
    Save-Text ([pscustomobject]@{ Manufacturer = $b.Manufacturer; Chemistry = $b.Chemistry; DesignCapacity_mWh = $b.DesignCapacity; FullChargeCapacity_mWh = $b.FullChargeCapacity; CycleCount = $b.CycleCount } | Format-List) "$Out\battery-capacity.txt"
} catch { "battery report: $_" | Out-File -Append -Encoding utf8 "$Out\errors.txt" }

# Audio, camera, sensors, input: covered by pnp-devices.csv; keep the camera stack visible
Save-Text ($rows | Where-Object { $_.Class -in 'Camera', 'MEDIA', 'Sensor', 'Biometric', 'HIDClass', 'SurfaceSystemManagement', 'System' -and $_.HardwareIds -match 'ACPI\\|HID\\|PCI\\|INTC|MSHW' } | Select Class, FriendlyName, Service, @{n='Id';e={($_.HardwareIds -split ' \| ')[0]}} | Sort Class, Id | Format-Table -AutoSize) "$Out\platform-devices.txt"


"done: $Out ; private: $Private"
