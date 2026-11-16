# Control socket (`yutu-mux ctl`)

Drive the **running** app from a terminal, a script, or an AI agent
with semantic commands instead of synthesized clicks. Every command
goes through the same `Message` the UI sends, so the app behaves
exactly as if a person clicked (same-song no-op, queue replacement,
seek holds all apply).

## Enable it

The listener is opt-in. Start the app with either:

```sh
yutu-mux --control
YUTU_CONTROL=1 yutu-mux        # or: YUTU_CONTROL=1 cargo run
```

It listens on a Unix socket, owner-only (`0600`) inside a private
(`0700`) directory:

- `$XDG_RUNTIME_DIR/yutu-mux/control.sock` (Linux), else
- `$TMPDIR/yutu-mux-$USER/control.sock` (macOS / no runtime dir).

`YUTU_CONTROL_SOCKET=/path/to.sock` overrides both (app and client).
There is no TCP listener and no Windows support.

## Use it

```sh
yutu-mux ctl ping
yutu-mux ctl state                       # route, now playing, position, window
yutu-mux ctl --wait search daft punk     # waits, then prints the results
yutu-mux ctl play-result 0
yutu-mux ctl seek 60
yutu-mux ctl nav settings
yutu-mux ctl resize 900 700
yutu-mux ctl screenshot shot.png         # captures the real (main) window
yutu-mux ctl mini open                   # mini player: open|close|restore
yutu-mux ctl mini lyrics on              # its lyrics ticker
yutu-mux ctl mini screenshot mini.png    # captures the mini window
yutu-mux ctl help                        # full command list
```

Output is JSON on stdout; errors go to stderr. Exit codes: `0` ok,
`1` the app refused or is unreachable, `2` usage error.

`quit` saves the session (queue, position, EQ) and closes the app.

### Hyprland

Tiled windows ignore resize requests, so under Hyprland:

```sh
yutu-mux ctl float on      # float the app window (like a Super+T bind)
yutu-mux ctl resize 900 700
yutu-mux ctl float off     # back to tiled (also: `ctl tile`)
yutu-mux ctl float         # toggle
```

`float` and `resize` go through `hyprctl`, targeting the app's own
main window, never the focused one and never the mini player. `resize` on a tiled
window fails with a hint instead of silently doing nothing. Other
tiling compositors (Sway, niri) may still ignore `resize`;
`state.window` always reports the real size.

`--wait` polls `state` until `busy` is false (search, track loading,
artist/album pages) and then prints the search results (after
`search`) or the state (anything else). `--timeout <secs>` bounds it
(default 20).

## Remote sessions

The socket is local. To drive the app on your desktop from another
machine, forward the socket over SSH and point the client at it:

```sh
ssh -L /tmp/yutu.sock:/run/user/1000/yutu-mux/control.sock desktop
YUTU_CONTROL_SOCKET=/tmp/yutu.sock yutu-mux ctl state
```

Screenshot paths are written **on the machine running the app**.

## Wire format

One JSON object per line each way, so any client works:

```sh
echo '{"args":["state"]}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/yutu-mux/control.sock
# {"ok":true,"data":{...}}   or   {"ok":false,"error":"..."}
```

`args` are the same words the CLI takes. A connection may send many
requests; each gets one reply line in order.
