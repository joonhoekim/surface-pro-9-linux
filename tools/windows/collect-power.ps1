# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Power baseline on Windows: sleep states, Modern Standby (sleepstudy) and energy reports. Read-only.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\run-elevated.ps1 tools\windows\collect-power.ps1
# Output: private/windows/power/ (reports carry machine name and device instance paths; summarize by hand into docs).
param([string]$Out, [int]$Days = 7)
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'private\windows\power' }
New-Item -ItemType Directory -Force $Out | Out-Null

powercfg /a | Out-File -Encoding utf8 (Join-Path $Out 'powercfg-a.txt')
powercfg /sleepstudy /duration $Days /output (Join-Path $Out 'sleepstudy.html') | Out-Null
powercfg /sleepstudy /duration $Days /xml /output (Join-Path $Out 'sleepstudy.xml') | Out-Null
powercfg /systempowerreport /duration $Days /output (Join-Path $Out 'systempowerreport.html') | Out-Null
powercfg /getactivescheme | Out-File -Encoding utf8 (Join-Path $Out 'scheme.txt')
powercfg /qh | Out-File -Encoding utf8 (Join-Path $Out 'scheme-all.txt')
powercfg /devicequery wake_armed | Out-File -Encoding utf8 (Join-Path $Out 'wake-armed.txt')
powercfg /requests | Out-File -Encoding utf8 (Join-Path $Out 'requests.txt')
Get-ChildItem $Out | Select-Object Name, Length | Format-Table -AutoSize | Out-String
