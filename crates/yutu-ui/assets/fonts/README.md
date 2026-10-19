# Fonts: Inter (OFL 1.1), static v4.1

`Inter-Regular.ttf`, `Inter-Medium.ttf`, `Inter-Bold.ttf` are verbatim
files from the Inter 4.1 release (https://github.com/rsms/inter):
the Text optical size, which suits UI body and small labels.
License: SIL Open Font License 1.1, full text in `OFL.txt` next to
the fonts (also at https://github.com/rsms/inter/blob/master/LICENSE.txt).

Why bundled: Iced resolves its default sans-serif through the system
font database, which picks visibly different (and on some macOS setups
worse-rendered) faces per machine. Shipping Inter makes body, Medium
button labels, and Bold headers render from the same outlines on Linux
and macOS. Glyphs Inter lacks (for example CJK) still fall back to
system fonts per glyph, so nothing renders as tofu.
Loaded once at startup in `yutu_ui::run` (`iced::application::font`);
the Settings Appearance section picks the family.
