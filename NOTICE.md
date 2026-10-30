# Third-party notices (yutu-mux)

yutu-mux sources are MIT, see `LICENSE`. The built app combines the
following third-party works. Keep this file accurate when adding deps.

## Fonts: Inter (OFL-1.1)

- Files: `crates/yutu-ui/assets/fonts/*.ttf`
- Copyright (c) 2016 The Inter Project Authors.
- License: SIL Open Font License 1.1, full text in
  `crates/yutu-ui/assets/fonts/OFL.txt`.

## Icons: Lucide (ISC, pinned 0.469.0)

- Files: `crates/yutu-ui/assets/icons/*.svg`, verbatim from `lucide-static`
  (https://lucide.dev), stripped only of the leading license HTML comment.
- License: ISC. Copyright (c) for Lucide contributors.
- Original license: https://github.com/lucide-icons/lucide/blob/main/LICENSE
- Full ISC text:

```
ISC License

Copyright (c) for Lucide contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

## Rust crates (permissive + weak copyleft)

- MIT / MIT OR Apache-2.0: `iced`, `tokio`, `reqwest`, `serde`,
  `serde_json`, `rusqlite` (SQLite itself is public domain),
  `ytmapi-rs`, `resvg`, `fontdb`, `mimalloc`, `num-traits`, `zbus`.
- BSD-3-Clause: `tiny-skia`.
- Apache-2.0: `cpal`.
- MPL-2.0 (weak file-level copyleft, notice only): `symphonia-*`
  (via `rodio` `symphonia-all`). MPL files stay MPL, compatible with
  MIT distribution.
- LGPL-2.1 shim: `libmpv2` (Rust crate) over the system library below.

## System library: libmpv (GPL-2.0-or-later, default build only)

- The default-feature build (`mpv` feature, on by default) dynamically
  links the system `libmpv` (GPL-2.0-or-later) at runtime. Distributed
  default binaries are therefore GPL-affected combined works.
- The `--no-default-features` build (rodio-only) is GPL-free.
- Install: `sudo pacman -S mpv` / `sudo apt install libmpv-dev` /
  `brew install mpv`.

## Runtime sidecar: yt-dlp (Unlicense, unlinked)

- `yt-dlp` is invoked as an external subprocess (`--get-url` for
  streaming resolution, `-o -` memory-only fetch fallback). It is never
  linked, so it has no license contagion. Not bundled in releases.
