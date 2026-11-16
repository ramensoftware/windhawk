# Installer for the auto-update-mods build

`build-installers.ps1` makes the release setups of this fork from the official,
signed Windhawk installers of the same version (2.0 alpha 8):

- `windhawk_setup.exe`
- `windhawk_setup_offline.exe` (includes the WebView2 runtime)

Each one is the official installer with this build's `windhawk-ui.exe`,
`windhawk-core.dll` and `windhawk-cli.exe` stored in it in place of the official
files. Running it installs Windhawk exactly like the official setup does - the same
pages, the Windhawk service, permissions, shortcuts and uninstaller - with this build's
files already in place. There is no second installer and nothing is replaced afterwards.

```powershell
.\build-installers.ps1 -FilesDir <folder with the three built files>
```

Needs Python 3 and 7-Zip. The official installers are downloaded from
https://github.com/ramensoftware/windhawk/releases and their signatures checked first.

`nsis_repack.py` does the rebuilding: it replaces only the installer file with the same
name and exactly the official contents (the UI's 32-bit `windhawk-core.dll` stays as
it is), lays out the stored data again, fixes the data offsets in the NSIS script
header and writes a new NSIS CRC. The script itself is not changed. The rebuilt setups
are not code-signed, so Windows SmartScreen may warn before they run.
