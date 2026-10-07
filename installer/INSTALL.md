# Installing the Sigstore tab

Run in PowerShell as administrator, from the folder you unzipped:

```powershell
mkdir "C:\Program Files\sigstore-shell"
copy sigstore_shell_ext.dll, register.ps1, unregister.ps1 "C:\Program Files\sigstore-shell\"
& "C:\Program Files\sigstore-shell\register.ps1"
```

Open the Properties of an `.exe` and look for the Sigstore tab. To remove it, run `unregister.ps1`, then restart Explorer before deleting the DLL.

`sigstore-shell.iss` is an Inno Setup script that does the same; build it on Windows.

Check the download first: `gh attestation verify sigstore_shell_ext.dll --repo non7top/sigstore-shell`.
