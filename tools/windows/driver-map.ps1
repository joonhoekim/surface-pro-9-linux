# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Map each present device to its driver package in the DriverStore (INF, folder, files).
# Output: hw/windows/driver-packages.txt. The packages themselves stay in the DriverStore (not copied into the repo).
param([string]$Out)
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'hw\windows\driver-packages.txt' }

$store = Join-Path $env:windir 'System32\DriverStore\FileRepository'
$byInf = @{}
Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.InfName -like 'oem*.inf' -and $_.DeviceName } | ForEach-Object {
    if (-not $byInf[$_.InfName]) { $byInf[$_.InfName] = @() }
    $byInf[$_.InfName] += $_
}
$lines = foreach ($inf in ($byInf.Keys | Sort-Object { [int]($_ -replace '\D', '') })) {
    $pub = Join-Path $env:windir "INF\$inf"
    # The published oemNN.inf is a copy of the original; find its DriverStore folder by content hash
    $h = (Get-FileHash $pub -Algorithm SHA256).Hash
    $dir = Get-ChildItem $store -Directory | Where-Object {
        Get-ChildItem $_.FullName -Filter *.inf -File | Where-Object { (Get-FileHash $_.FullName -Algorithm SHA256).Hash -eq $h }
    } | Select-Object -First 1
    $d = $byInf[$inf][0]
    "## $inf  ($($d.DriverProviderName) $($d.DriverVersion))"
    foreach ($x in ($byInf[$inf] | Sort-Object DeviceName -Unique)) { "  device: $($x.DeviceName)  [$($x.HardWareID)]" }
    if ($dir) {
        "  store:  $($dir.Name)"
        $files = Get-ChildItem $dir.FullName -Recurse -File | Where-Object { $_.Extension -notin '.cat', '.pnf' }
        "  files:  " + (($files | ForEach-Object { $_.FullName.Substring($dir.FullName.Length + 1) + " (" + $_.Length + ")" }) -join ', ')
    }
    ''
}
$lines | Out-File -Encoding utf8 $Out
"wrote $Out"
