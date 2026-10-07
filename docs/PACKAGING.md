# Packaging (Phase 6)

How a `0.2.0` release ships: Linux Flatpak + portable tarball (CI-built),
macOS `.dmg` notes (UNTESTED — the maintainer has no Mac).

## 1. Prerequisites

### Linux (tarball + local builds)

```sh
sudo apt-get update
sudo apt-get install -y libasound2-dev pkg-config libx11-dev \
  libwayland-dev libxkbcommon-dev libmpv-dev
```

Same set as `.github/workflows/ci.yml`, plus `libmpv-dev`: the `mpv`
cargo feature does not exist yet (`MpvBackend` is a Phase-5 follow-up),
but the system library is installed now so the future backend links
without workflow changes. Rust via `mise use -g rust@stable` (rust 1.99+).

### Flatpak tooling (Linux only)

```sh
flatpak remote-add --if-not-exists flathub \
  https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.freedesktop.Platform//24.08 \
  org.freedesktop.Sdk//24.08
flatpak install flathub org.flatpak.Builder
```

Runtime decision: `org.freedesktop.Platform//24.08` + matching SDK.
`23.08` is older/EOL-track; `24.08` is the current LTS-style target and
its SDK ships a Rust toolchain, so no `rust-stable` SDK extension is
needed. (If a newer LTS, e.g. `25.08`, becomes the Flathub default before
release, bump `runtime-version` in `flatpak/org.yutu_mux.YutuMux.yaml` —
nothing else changes.)

### macOS (UNTESTED)

Xcode command-line tools + a stable Rust toolchain. No other prerequisites
today (pure-Rust audio/UI; no Homebrew-only system deps).

## 2. Flatpak: offline-vendor step (required before build)

Flatpak builds are network-isolated, so crates.io sources must be staged
up front. Generated files are NEVER committed (`flatpak/vendor/`,
`flatpak/vendor-config.toml`, `flatpak/cargo-sources.json`); add them to
`.gitignore` (maintainer action).

Option A — local/dev builds, pure cargo (no extra tooling):

```sh
git checkout v0.2.0  # or the release tag under test
cargo vendor vendor > flatpak/vendor-config.toml
```

This writes `flatpak/vendor-config.toml` (a `[source.crates-io]
replace-with = "vendored-sources"` snippet pointing at `directory =
"vendor"`) plus the `vendor/` tree. The manifest's first build command
copies that snippet to `.cargo/config.toml` inside the build dir and then
runs `cargo --offline build --release --locked`. If you skip this step,
that `cp` fails loudly — by design.

Option B — Flathub submission (network at download time, offline at build
time): use `flatpak-cargo-generator.py` from `flatpak-builder-tools` to
turn `Cargo.lock` into downloadable archive sources, and inline the
generated entries into the manifest per that tool's README:

```sh
python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
```

Keep the generated file out of git either way.

## 3. Flatpak: build / test / install

```sh
# Build (from the repo root, after the vendor step above):
flatpak-builder --force-clean build-dir flatpak/org.yutu_mux.YutuMux.yaml

# Smoke-test without installing:
flatpak-builder --run build-dir flatpak/org.yutu_mux.YutuMux.yaml yutu-mux

# Install per-user and run:
flatpak-builder --user --install --force-clean build-dir \
  flatpak/org.yutu_mux.YutuMux.yaml
flatpak run org.yutu_mux.YutuMux

# Verify the MPRIS name is owned on the session bus:
gdbus introspect --session \
  --dest org.mpris.MediaPlayer2.yutu_mux \
  --object-path /org/mpris/MediaPlayer2 | head
```

Sandbox notes: graphics via Wayland with X11 fallback
(`--socket=wayland`, `--socket=fallback-x11`); audio via the PulseAudio
socket, which also covers PipeWire-pulse and ALSA clients (the runtime
ships `alsa-plugins-pulse` — there is no separate ALSA flatpak socket);
`--share=network` is required (streaming + Innertube API);
`--own-name=org.mpris.MediaPlayer2.yutu_mux` is the only session-bus
grant (no blanket `--socket=session-bus`). GPU acceleration via
`--device=dri`.

Desktop file: `flatpak/org.yutu_mux.YutuMux.desktop`
(`Exec=yutu-mux`, `Icon=org.yutu_mux.YutuMux`) is installed by the
manifest to `/app/share/applications/`. Icon expectation: scalable SVG at
`flatpak/org.yutu_mux.YutuMux.svg`, installed to
`/app/share/icons/hicolor/scalable/apps/` — real artwork has NOT landed
yet, so the manifest installs it conditionally (`if test -f ...`) and the
app currently ships without an icon. Supplying the SVG needs no manifest
change. Follow-up before Flathub submission: AppStream metainfo
(`org.yutu_mux.YutuMux.metainfo.xml`).

## 4. Portable tarball

`.github/workflows/release.yml` runs on every `v*` tag: checkout →
toolchain → apt deps → `cargo test --workspace` → feature checks
(`--all-features` compile for `innertube`, plus a non-blocking `mpv`
probe) → `cargo build --release` → stage + `tar -czf` → upload to the
GitHub Release via `softprops/action-gh-release@v2` (MIT-licensed action).

Tarball `yutu-mux-x86_64-unknown-linux-gnu.tar.gz` contents:

- `yutu-mux` (release binary, x86_64 Linux)
- `README.md`
- `LICENSE` (if present in the repo)
- `flatpak/org.yutu_mux.YutuMux.yaml` + `flatpak/org.yutu_mux.YutuMux.desktop`
  (so tarball users can rebuild/install the Flatpak manifest)

Local repro of the packaging step (after `cargo build --release`):

```sh
set -eu
STAGE="stage/yutu-mux-x86_64-unknown-linux-gnu"
mkdir -p "$STAGE"
cp target/release/yutu-mux "$STAGE/"
cp README.md "$STAGE/"
if test -f LICENSE; then cp LICENSE "$STAGE/"; fi
cp flatpak/org.yutu_mux.YutuMux.yaml flatpak/org.yutu_mux.YutuMux.desktop "$STAGE/"
tar -czf yutu-mux-x86_64-unknown-linux-gnu.tar.gz -C stage yutu-mux-x86_64-unknown-linux-gnu
```

Note: the tarball binary is linked against system ALSA/X11/Wayland, so it
is "portable" across similar desktop distros, not static. Recipients need
the Section-1 libraries installed.

## 5. macOS `.dmg` — UNTESTED (maintainer has no Mac)

Honest status: the steps below are standard practice but have never been
run for this repo. Do NOT advertise a macOS release until someone with a
Mac walks through them and updates this doc.

```sh
# 1. Release build on the Mac:
cargo build --release

# 2. Assemble a minimal app bundle:
APP="YutuMux.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/yutu-mux "$APP/Contents/MacOS/YutuMux"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>YutuMux</string>
  <key>CFBundleIdentifier</key><string>dev.yutu-mux.YutuMux</string>
  <key>CFBundleName</key><string>YutuMux</string>
  <key>CFBundleVersion</key><string>0.2.0</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
</dict></plist>
PLIST

# 3. Ad-hoc sign (no Developer ID; Gatekeeper will still warn):
codesign --force --deep --sign - "$APP"

# 4a. Build the .dmg with create-dmg (MIT-licensed), or:
create-dmg "$APP" --dmg-title="YutuMux" .

# 4b. ... manually with system hdiutil (no third-party tool):
hdiutil create -volname YutuMux -srcfolder "$APP" -ov -format UDZO YutuMux.dmg
```

Notarization (paid Apple Developer ID, stapling, `notarytool`) is out of
scope until there is a Mac-owning maintainer; until then any `.dmg` is
ad-hoc-signed only.

## 6. Version-bump checklist (0.2.0)

There are FIVE `Cargo.toml` version fields (not four — the binary crate
counts too); today all read `0.1.0`:

1. `Cargo.toml` (binary `yutu-mux`)
2. `crates/yutu-core/Cargo.toml`
3. `crates/yutu-player/Cargo.toml`
4. `crates/yutu-ui/Cargo.toml`
5. `crates/yutu-desktop/Cargo.toml`

Then:

6. Add a `## 0.2.0` entry to `CHANGELOG.md`.
7. Commit, tag `git tag v0.2.0`, push tag — `release.yml` builds the
   tarball and attaches it to the GitHub Release automatically.
8. Regenerate the Flatpak vendor outputs (Section 2) from the tagged tree
   before `flatpak-builder`, and verify `flatpak run` + the MPRIS check.

## 7. License hygiene

Everything added here is MIT-clean: the manifest, desktop file, workflow,
and this doc contain no GPL tooling and vendor nothing. `create-dmg` is
MIT-licensed; `hdiutil`/`codesign` are macOS system tools, not bundled;
`flatpak-cargo-generator.py` (`flatpak-builder-tools`) is only ever run by
the maintainer, never checked in.
