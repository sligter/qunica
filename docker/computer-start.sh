#!/bin/sh
set -eu
mkdir -p "$HOME/workspace/artifacts" "$HOME/.qunica"
export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
openbox >/dev/null 2>&1 &
x11vnc -display :99 -localhost -forever -shared -nopw -rfbport 5900 >/dev/null 2>&1 &
x11vnc -display :99 -localhost -forever -shared -viewonly -nopw -rfbport 5901 >/dev/null 2>&1 &
exec node /opt/qunica/service.mjs
