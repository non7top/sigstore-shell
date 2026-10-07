#Requires -RunAsAdministrator
param([string]$Dll = (Join-Path $PSScriptRoot 'sigstore_shell_ext.dll'))

$ErrorActionPreference = 'Stop'
$Dll = (Resolve-Path $Dll).Path
$p = Start-Process -FilePath "$env:SystemRoot\System32\regsvr32.exe" -ArgumentList '/s', '/u', "`"$Dll`"" -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "regsvr32 /u failed with exit code $($p.ExitCode)" }
Write-Host 'Unregistered. Explorer may keep the DLL loaded until it restarts; then you can delete it.'
