#!/bin/bash
# Smoke test for octocrab-tauri: open the control center under Xvfb and
# capture a screenshot of the root window.
set -u
source /home/z/sysroot/env.sh
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" /tmp/xdg-dbus

cd /home/z/octocrab-suite

SHOT=/tmp/octocrab-tauri-xvfb.png
rm -f "$SHOT"

# ImageMagick's importer ships as import-im7.q16 in the sysroot
grab() {
  if command -v import >/dev/null 2>&1; then
    import -display :77 -window root "$SHOT"
  else
    import-im7.q16 -display :77 -window root "$SHOT"
  fi
}

dbus-run-session -- bash -c '
  Xvfb :77 -screen 0 1440x900x24 &
  XVFB_PID=$!
  sleep 2
  DISPLAY=:77 ./target/debug/octocrab-tauri 2>/tmp/octocrab-tauri-app.log &
  APP_PID=$!
  sleep 12
  DISPLAY=:77 xwd -root -silent /tmp/octocrab-tauri-xwd.dump 2>/dev/null
  kill $APP_PID $XVFB_PID 2>/dev/null
  wait $APP_PID 2>/dev/null
'

grab || echo "import failed (exit $?)"

# Fallback: convert the xwd dump if import produced nothing
if [ ! -s "$SHOT" ] && [ -s /tmp/octocrab-tauri-xwd.dump ]; then
  if command -v convert >/dev/null 2>&1; then
    convert /tmp/octocrab-tauri-xwd.dump "$SHOT"
  else
    convert-im7.q16 /tmp/octocrab-tauri-xwd.dump "$SHOT"
  fi
fi

echo "smoke done: $(ls -la "$SHOT" 2>/dev/null || echo NO_SCREENSHOT)"
echo "--- app log (first 40 lines) ---"
head -40 /tmp/octocrab-tauri-app.log 2>/dev/null || echo "(no app log)"
