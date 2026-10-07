# Troubleshooting

## No audio device

`RodioBackend::try_new()` fails headless. The app runs with a status note;
tests skip (`SKIP: no audio device`). On real hardware check PipeWire/ALSA
(`pactl info`, `aplay -l`) and the `pulseaudio` socket inside Flatpak.

## Stream 403 / PO token

See `docs/PO_TOKEN.md`. Try the `android_vr` fallback first, then install
`rustypipe-botguard`. Save redacted payloads via `save_report()` into
`reports/` and file an issue with the client label + timestamp.

## Innertube breakage (empty lists, parse errors)

Pin the working `ytmapi-rs` version from `Cargo.lock`, attach the
`reports/*.json` payload, and note the query + time. Never attach cookies.

## GUI won't start

Needs a display (`$DISPLAY` or Wayland socket) + wgpu-capable GPU. Run
`cargo run` from a desktop session, not SSH without forwarding.

## Cookie expired

Re-export `cookies.txt` from a fresh private window (see `docs/AUTH.md`),
re-import in Settings, restart.
