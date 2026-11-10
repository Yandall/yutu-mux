# Changelog

<!-- Until we release 0.1.0 we don't need to add anything here. After that we will remove that comment and start writing the changes for each version. -->

## Unreleased

- Windows support: self-contained package (app plus mpv/ffmpeg DLLs and
  the yt-dlp.exe sidecar beside the exe, no manual dependencies).
  New `windows-zip-setup` release job builds the zip plus an Inno Setup
  per-user installer on every tag; install via the Setup exe,
  `packaging/windows/install.ps1`, or the Scoop manifest in
  `packaging/windows/scoop/`. App data lives under `%APPDATA%\yutu-mux`.
  Linux and macOS behavior is unchanged (system dependencies via
  `packaging/install.sh`, nothing bundled).
