#!/bin/sh
set -eu
mkdir -p "$HOME/workspace/artifacts" "$HOME/.qunica"
export DISPLAY=:99
log_dir="${TMPDIR:-/tmp}/qunica-desktop"
mkdir -p "$log_dir"
chmod 700 "$log_dir"
pids=""
trap 'for pid in $pids; do kill "$pid" 2>/dev/null || true; done' EXIT
trap 'exit 1' INT TERM

# Note: Xvfb creates its socket asynchronously. A fixed sleep or parallel
# launch can leave working VNC connections showing only a black X root window.
wait_ready() {
    label=$1
    pid=$2
    shift 2
    attempt=0
    while [ "$attempt" -lt 100 ]; do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "$label exited during startup; see $log_dir" >&2
            return 1
        fi
        if "$@" >/dev/null 2>&1; then return 0; fi
        attempt=$((attempt + 1))
        sleep 0.1
    done
    echo "$label did not become ready; see $log_dir" >&2
    return 1
}

Xvfb :99 -screen 0 1280x800x24 -nolisten tcp -noreset >"$log_dir/xvfb.log" 2>&1 &
x_pid=$!
pids="$pids $x_pid"
wait_ready 'X display' "$x_pid" xdotool getdisplaygeometry

openbox >"$log_dir/openbox.log" 2>&1 &
wm_pid=$!
pids="$pids $wm_pid"
wait_ready 'Window manager' "$wm_pid" sh -c 'xprop -root _NET_SUPPORTING_WM_CHECK | grep -q "window id #"'
# Apply after a persistent X client exists, otherwise X resets these settings
# when the last readiness probe disconnects. No physical monitor can wake it.
xset s off
xset s noblank
xset -dpms >"$log_dir/power.log" 2>&1 || true

# Keep the idle desktop distinguishable from a broken/blank viewer. Chromium
# remains lazy: prelaunching it here would compete with the MCP-owned profile.
xsetroot -solid '#243447'
xmessage -center -title 'Qunica computer' -bg '#243447' -fg white -buttons 'OK:0' \
    'Qunica computer is ready.

The browser opens when an Agent uses browser tools.
Take control in Qunica to interact with this desktop.' \
    >"$log_dir/welcome.log" 2>&1 &
pids="$pids $!"

for port in 5900 5901; do
    view_only=""
    if [ "$port" = 5901 ]; then view_only=-viewonly; fi
    x11vnc -display :99 -localhost -forever -shared $view_only -nopw -rfbport "$port" \
        >"$log_dir/vnc-$port.log" 2>&1 &
    vnc_pid=$!
    pids="$pids $vnc_pid"
    wait_ready "VNC $port" "$vnc_pid" python3 -c \
        'import socket,sys; socket.create_connection(("127.0.0.1",int(sys.argv[1])),timeout=0.2).close()' "$port"
done
exec node /opt/qunica/service.mjs
