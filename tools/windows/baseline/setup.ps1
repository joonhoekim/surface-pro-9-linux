# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Prepare the baseline kit: build sp9probe.exe, create the Python venv, find ffmpeg, fetch the Bluetooth WPR profile.
# Nothing here changes system settings. Re-run any time; each step is skipped when already done (-Force rebuilds).
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\baseline\setup.ps1 [-Force]
param([switch]$Force)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

New-Item -ItemType Directory -Force $Kit.Out, $Kit.Data | Out-Null

# 1. sp9probe.exe (.NET Framework csc, C# 5, WinForms)
$src = Join-Path $PSScriptRoot 'sp9probe.cs'
if ($Force -or -not (Test-Path $Kit.Probe) -or (Get-Item $src).LastWriteTime -gt (Get-Item $Kit.Probe).LastWriteTime) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $csc /nologo /optimize+ /target:exe /platform:x64 /codepage:65001 /out:$($Kit.Probe) `
        /r:System.Windows.Forms.dll /r:System.Drawing.dll $src
    if ($LASTEXITCODE) { throw "csc failed ($LASTEXITCODE)" }
}
"probe   : $($Kit.Probe)"

# 2. Python venv (python -I ignores user site-packages, so packages live in a venv)
if ($Force -or -not (Test-Path $Kit.Python)) {
    $base = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python313\python.exe'
    if (-not (Test-Path $base)) { $base = (Get-Command python -ErrorAction SilentlyContinue).Source }
    if (-not $base) { throw 'Python 3 not found: winget install Python.Python.3.13' }
    & $base -I -m venv (Split-Path (Split-Path $Kit.Python))
}
& $Kit.Python -I -m pip install --disable-pip-version-check -q hidapi numpy sounddevice
if ($LASTEXITCODE) { throw 'pip install failed' }
"python  : $($Kit.Python) (" + (& $Kit.Python -I -c "import hid,numpy,sounddevice;print('hidapi numpy', numpy.__version__, 'sounddevice', sounddevice.__version__)") + ')'

# 3. ffmpeg (winget Gyan.FFmpeg)
if (-not (Get-Ffmpeg)) {
    winget install --id Gyan.FFmpeg -e --silent --accept-package-agreements --accept-source-agreements | Out-Null
}
"ffmpeg  : $(Get-Ffmpeg)"

# 4. Microsoft busiotools Bluetooth WPR profile (MIT)
if ($Force -or -not (Test-Path $Kit.Wprp)) {
    try {
        Invoke-WebRequest 'https://raw.githubusercontent.com/microsoft/busiotools/master/bluetooth/tracing/BluetoothStack.wprp' -UseBasicParsing -OutFile $Kit.Wprp
    } catch { "wprp    : download failed ($($_.Exception.Message)); BLE trace will be skipped" }
}
if (Test-Path $Kit.Wprp) { "wprp    : $($Kit.Wprp)" }
