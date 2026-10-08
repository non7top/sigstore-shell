#Requires -RunAsAdministrator
# Registers sigstore_shell_ext.dll for all users. Run from the folder that will keep the DLL.
param([string]$Dll = (Join-Path $PSScriptRoot 'sigstore_shell_ext.dll'))

$ErrorActionPreference = 'Stop'
$Dll = (Resolve-Path $Dll).Path
$p = Start-Process -FilePath "$env:SystemRoot\System32\regsvr32.exe" -ArgumentList '/s', "`"$Dll`"" -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "regsvr32 failed with exit code $($p.ExitCode)" }
Write-Host "Registered $Dll. Open Properties on an .exe to see the Provenance tab."
Write-Host 'If the tab does not show, restart Explorer: Stop-Process -Name explorer'
