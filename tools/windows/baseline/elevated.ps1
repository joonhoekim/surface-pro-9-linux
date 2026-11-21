# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Elevated parts of the baseline kit. Run through tools\windows\run-elevated.ps1 (silent UAC); every action is read-only
# to system configuration (ETW tracing, HID reads, power reports).
#   -Action bt-start  -Dir DIR    start the Microsoft BluetoothStack WPR profile (busiotools) into memory/file mode
#   -Action bt-stop   -Dir DIR    stop it and write DIR\bt.etl
#   -Action bt-cancel             drop a running trace without saving
#   -Action hidlog    -Dir DIR    hidlog.py for the THC/KIP/BLE collections that open only elevated (Col04, Col07, ...)
#   -Action sleepstudy -Dir DIR   powercfg /sleepstudy (html + xml) and /a for the last day
param([Parameter(Mandatory)][string]$Action, [string]$Dir, [string]$Select = 'thc,ble,kip')
$ErrorActionPreference = 'Continue'   # native tools write to stderr; 'Stop' would turn that into a terminating error in PS 5.1
. (Join-Path $PSScriptRoot 'common.ps1')
$inst = 'sp9baseline'
if ($Dir) { New-Item -ItemType Directory -Force $Dir | Out-Null }

switch ($Action) {
    'bt-start' {
        if (-not (Test-Path $Kit.Wprp)) { 'bt-start: no BluetoothStack.wprp (run setup.ps1)'; exit 2 }
        cmd /c "wpr.exe -cancel -instancename $inst >nul 2>&1"
        wpr.exe -start "$($Kit.Wprp)!BluetoothStack" -filemode -instancename $inst
        "bt-start exit $LASTEXITCODE"
        exit $LASTEXITCODE
    }
    'bt-stop' {
        wpr.exe -stop (Join-Path $Dir 'bt.etl') -instancename $inst
        "bt-stop exit $LASTEXITCODE"
        exit $LASTEXITCODE
    }
    'bt-cancel' { wpr.exe -cancel -instancename $inst; exit 0 }
    'hidlog' {
        & $Kit.Python -I (Join-Path $PSScriptRoot 'hidlog.py') --out $Dir --select $Select
        exit $LASTEXITCODE
    }
    'sleepstudy' {
        powercfg /sleepstudy /duration 1 /output (Join-Path $Dir 'sleepstudy.html') | Out-Null
        powercfg /sleepstudy /duration 1 /xml /output (Join-Path $Dir 'sleepstudy.xml') | Out-Null
        powercfg /a | Out-File -Encoding utf8 (Join-Path $Dir 'powercfg-a.txt')
        'sleepstudy done'
    }
    default { "unknown action $Action"; exit 1 }
}
