# Changelog

<!-- Until we release 0.1.0 we don't need to add anything here. After that we will remove that comment and start writing the changes for each version. -->

## Unreleased

- Now Playing drawer snaps back to the top (hero) on every song
  change while open, instead of keeping the old song's scroll depth;
  lyrics follow re-arms from scratch and centers the verse once it
  lands.

- Default font is now bundled Adwaita Sans (GNOME's Inter variation,
  OFL-1.1, variable v51.0): one `AdwaitaSans-Regular.ttf` covers body,
  Medium, and Bold weights, replacing the three Inter statics. Stored
  `font_family` prefs migrate automatically (legacy `inter` parses to
  the new default); the picker head reads "Adwaita Sans (bundled)".

- Selector menus: closed picker faces now carry an input-like border
  (new `select_face` button style) so they never read as plain text,
  and the open card caps at 320px with a slim scrollbar instead of
  stretching past the viewport for long lists (system fonts).

- Security barrier: least-privilege allowlists so the app only does what
  it is supposed to, even if a sidecar or server row is compromised.
  New `yutu-core::security` module (std-only, always on): strict 11-char
  video ids, safe server ids, `https` googlevideo-only stream URLs and
  YouTube-image-only artwork URLs, URL redaction and stderr truncation,
  D-Bus seek and volume clamping, plus search, cookie, and artwork size
  caps. Sidecars no longer load from the current directory (PATH and exe
  dir only, executable bit required), all HTTP clients carry a 15s
  timeout with limited redirects, `fetch_bytes` and `warm_stream_url`
  reject off-domain hosts, `MpvBackend::load_url` accepts only
  googlevideo and `load_file` only temp-dir files (exclusive-create
  spills), and `save_artwork_in` caps at 10 MiB. Off-domain sidecar
  output is rejected fail-closed.

- Windows support: self-contained package (app plus libmpv-2.dll and
  the yt-dlp.exe sidecar beside the exe, no manual dependencies).
  New `windows-zip-setup` release job builds the zip plus an Inno Setup
  per-user installer on every tag; install via the Setup exe,
  `packaging/windows/install.ps1`, or the Scoop manifest in
  `packaging/windows/scoop/`. The same zip can also be built on Linux
  through Docker (`packaging/windows/build-windows-docker.sh`); the
  Setup exe still needs Windows. The Windows exe carries the app icon
  (plus the live window title-bar icon), defaults to the DX12 GPU
  backend (the driver probe for the other backends hangs on some
  machines with no window and no error; an
  explicit `WGPU_BACKEND` still wins), and release builds open no
  console window. App data lives under
  `%APPDATA%\yutu-mux`. Linux and macOS behavior is unchanged (system
  dependencies via `packaging/install.sh`, nothing bundled).
