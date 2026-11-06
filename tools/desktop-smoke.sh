#!/usr/bin/env bash
# Exercises the real UI with a separate data directory and D-Bus session.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
export SAIFORI_LOG_DIR="${SAIFORI_LOG_DIR:-$PWD/target/desktop-smoke-logs/$$}"
mkdir -p "$SAIFORI_LOG_DIR"
SAIFORI_LOG_DIR=$(cd "$SAIFORI_LOG_DIR" && pwd)
exec > >(tee "$SAIFORI_LOG_DIR/desktop.log") 2>&1
printf 'Desktop diagnostic directory: %s\n' "$SAIFORI_LOG_DIR"
task_state=$(mktemp -d)
cleanup() {
    result=$?
    if (( result != 0 )); then
        echo "Desktop smoke test failed (exit $result). Application log:" >&2
        if [[ -f "$SAIFORI_LOG_DIR/application.log" ]]; then cat "$SAIFORI_LOG_DIR/application.log" >&2; fi
    fi
    rm -rf "$task_state"
    exit "$result"
}
trap cleanup EXIT
export XDG_DATA_HOME="$task_state/data"
export XDG_CONFIG_HOME="$task_state/config"
export XDG_CACHE_HOME="$task_state/cache"
export SAIFORI_SCREENSHOT_DIR="${SAIFORI_SCREENSHOT_DIR:-$SAIFORI_LOG_DIR/screenshots}"
export GTK_A11Y=atspi GSK_RENDERER=cairo
export LC_ALL=C.UTF-8 LANGUAGE=en
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$SAIFORI_SCREENSHOT_DIR"
for dependency in xvfb-run dbus-run-session import; do
    command -v "$dependency" >/dev/null || { echo "Missing dependency: $dependency" >&2; exit 1; }
done
export SAIFORI_BINARY="${SAIFORI_BINARY:-$PWD/target/release/saifori}"
[[ -x "$SAIFORI_BINARY" ]] || { echo "Executable not found: $SAIFORI_BINARY" >&2; exit 1; }
{
    uname -sr
    /usr/bin/python3 --version
    pkg-config --modversion gtk4 libadwaita-1 libsecret-1 || true
    /usr/bin/python3 -c 'import pyatspi; print("AT-SPI Python bindings: available")'
} > "$SAIFORI_LOG_DIR/runtime.log" 2>&1
xvfb-run -e "$SAIFORI_LOG_DIR/xvfb.log" -a -s '-screen 0 1600x1000x24 -nolisten tcp' dbus-run-session -- bash -eu -o pipefail <<'SESSION'
"$SAIFORI_BINARY" > "$SAIFORI_LOG_DIR/application.log" 2>&1 &
app_pid=$!
cleanup_session() {
    result=$?
    if (( result != 0 )); then
        import -window root "$SAIFORI_SCREENSHOT_DIR/failure.png" 2>> "$SAIFORI_LOG_DIR/capture.log" || true
    fi
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
    exit "$result"
}
trap cleanup_session EXIT
for attempt in {1..50}; do
    [[ -f "$XDG_DATA_HOME/saifori/ledger.sqlite3" ]] && break
    kill -0 "$app_pid"
    sleep 0.1
done
sleep 1
kill "$app_pid"
wait "$app_pid" || true
/usr/bin/python3 - <<'PY'
import os, sqlite3
with sqlite3.connect(os.environ['XDG_DATA_HOME']+'/saifori/ledger.sqlite3') as connection:
    connection.execute("INSERT OR REPLACE INTO settings(name,value) VALUES('test_mode','1')")
PY
"$SAIFORI_BINARY" >> "$SAIFORI_LOG_DIR/application.log" 2>&1 &
app_pid=$!
/usr/bin/python3 tools/desktop-probe.py
kill "$app_pid"
wait "$app_pid" || true
/usr/bin/python3 - <<'PY'
import os, sqlite3
with sqlite3.connect(os.environ['XDG_DATA_HOME']+'/saifori/ledger.sqlite3') as connection:
    assert connection.execute('SELECT COUNT(*) FROM transactions').fetchone()[0] == 0
print('PASS: test transactions did not enter the persistent ledger')
PY
if grep -Ei 'panicked at|Gtk-CRITICAL|GLib-CRITICAL' "$SAIFORI_LOG_DIR/application.log"; then exit 1; fi
SESSION
