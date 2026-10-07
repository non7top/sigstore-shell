#!/bin/sh
# Runs inside the wine container: registers the DLL, loads it through COM and, with a display, drives the page.
set -eu

XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_RUNTIME_DIR

# xvfb-run hangs when it is PID 1 in a container, so start the display directly.
case " $* " in
*" --ui "*)
    Xvfb :99 -screen 0 1280x1024x24 -nolisten tcp &
    xvfb_pid=$!
    trap 'kill "$xvfb_pid"' EXIT
    DISPLAY=:99
    export DISPLAY
    sleep 2
    ;;
esac

target=/cargo-target/x86_64-pc-windows-gnu/release
dest="$WINEPREFIX/drive_c/sigstore-shell"
mkdir -p "$dest"
cp "$target/sigstore_shell_ext.dll" "$target/examples/smoke.exe" /app/dist/fixture.exe "$dest/"

wineboot -u
dll='C:\sigstore-shell\sigstore_shell_ext.dll'
handler='HKLM\Software\Classes\exefile\shellex\PropertySheetHandlers\SigstoreShell'
clsid='HKLM\Software\Classes\CLSID\{fbcd8210-9f9c-4b07-900a-ad12500a4363}\InprocServer32'

wine regsvr32 /s "$dll"
wine reg query "$handler"
wine reg query "$clsid"
wine reg query 'HKLM\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved' | grep -i fbcd8210

wine 'C:\sigstore-shell\smoke.exe' "$dll" 'C:\sigstore-shell\fixture.exe' "$@"

wine regsvr32 /s /u "$dll"
if wine reg query "$handler" >/dev/null 2>&1; then
    echo "FAIL handler key still present after unregister"
    exit 1
fi
echo "PASS unregister removed the handler key"
