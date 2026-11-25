<!-- release-files -->
## Files in this release

| File | Platform | Architecture | What it is |
|---|---|---|---|
| `sigstore-shell-cli.exe` | Windows | x86-64 | Command line check (built for `x86_64-pc-windows-gnu`) |
| `sigstore-shell-cli` | Linux | x86-64 | Command line check (ELF, dynamically linked against glibc) |
| `sigstore-shell-Setup-<version>.exe` | Windows | x86-64 | Installer for the Explorer tab (needs administrator) |
| `sigstore-shell-ext-<version>-windows-x64.zip` | Windows | x86-64 | The DLL with `register.ps1` and `unregister.ps1` |
| `sigstore_shell_ext.dll` | Windows | x86-64 | The Explorer extension on its own |
| `SHA256SUMS` | any | any | SHA-256 of the files above |
| `*.cosign.bundle` | any | any | cosign signature bundle for the file with the same name |

There is no arm64, 32-bit or macOS build. Every binary reports the release version and the commit it was built from.
