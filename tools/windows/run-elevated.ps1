# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Run a PowerShell script elevated and print its output here.
# UAC on this machine elevates without asking (ConsentPromptBehaviorAdmin = 0); Windows sudo is disabled.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\run-elevated.ps1 <script.ps1> [args...]
param([Parameter(Mandatory)][string]$Script, [Parameter(ValueFromRemainingArguments)][string[]]$Rest)
$Script = (Resolve-Path $Script).Path
$log = Join-Path $env:TEMP ("run-elevated-{0}.log" -f [guid]::NewGuid())
# Parameter names (-Name) must stay unquoted or the target script binds them as positional strings
$argStr = if ($Rest) { ($Rest | ForEach-Object { if ($_ -match '^-\w+:?$') { $_ } else { "'" + ($_ -replace "'", "''") + "'" } }) -join ' ' } else { '' }
$cmd = "& { Set-Location '$($PWD.Path)'; & '$Script' $argStr } *>&1 | Out-File -Encoding utf8 '$log'; exit `$LASTEXITCODE"
$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $cmd
if (Test-Path $log) { Get-Content $log; Remove-Item $log }
exit $p.ExitCode
