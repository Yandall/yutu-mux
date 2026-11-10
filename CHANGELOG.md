# Changelog

<!-- Until we release 0.1.0 we don't need to add anything here. After that we will remove that comment and start writing the changes for each version. -->

## Unreleased

- Windows support: self-contained package (app plus libmpv-2.dll and
  the yt-dlp.exe sidecar beside the exe, no manual dependencies).
  New `windows-zip-setup` release job builds the zip plus an Inno Setup
  per-user installer on every tag; install via the Setup exe,
  `packaging/windows/install.ps1`, or the Scoop manifest in
  `packaging/windows/scoop/`. The same zip can also be built on Linux
  through Docker (`packaging/windows/build-windows-docker.sh`); the
  Setup exe still needs Windows. The Windows exe carries the app icon
  and defaults to the DX12 GPU backend (the driver probe for the other
  backends hangs on some machines with no window and no error; an
  explicit `WGPU_BACKEND` still wins). App data lives under
  `%APPDATA%\yutu-mux`. Linux and macOS behavior is unchanged (system
  dependencies via `packaging/install.sh`, nothing bundled).
