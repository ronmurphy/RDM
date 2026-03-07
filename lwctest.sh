# labwc will write its socket to e.g. wayland-1
# Check: ls /run/user/$(id -u)/

# Terminal 2 — run RDM session inside it
export WAYLAND_DISPLAY=wayland-1
export XDG_SESSION_TYPE=wayland
rdm-start   # or just rdm-session directly
