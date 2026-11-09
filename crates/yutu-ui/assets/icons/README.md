# Icons: Lucide (ISC), pinned v0.469.0
Files here are verbatim SVGs from `lucide-static` (https://lucide.dev),
stripped only of the leading license HTML comment.
License: ISC: Copyright (c) for Lucide contributors; see
https://github.com/lucide-icons/lucide/blob/main/LICENSE.
The full ISC text is reproduced in the root `NOTICE.md`, which covers
these files for redistribution.
Rendered via Iced's `svg` widget, tinted with the app palette
(Lucide uses `stroke="currentColor"`, so single-color tinting works).
Transport exception: `play`, `pause`, `skip-back`, `skip-forward` are
filled derivations (`fill="currentColor"`, `stroke="none"`) so the
transport reads solid at small sizes. Their bars are filled `rect`s
(a stroked `line` with `stroke="none"` paints nothing) and the play
triangle is shifted one unit left (`5 3 19 12 5 21`) so the glyph
spans 5-19, optically centered like pause.
