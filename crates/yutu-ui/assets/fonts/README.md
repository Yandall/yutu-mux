# Fonts: Adwaita Sans (OFL 1.1), variable v51.0

`AdwaitaSans-Regular.ttf` is a verbatim file from the Adwaita Fonts
51.0 release (https://download.gnome.org/sources/adwaita-fonts/51/):
the upright variable font, covering Thin through Black in one file,
so body text, Medium button labels, and Bold headers all resolve
from these outlines. Italic is not bundled (the app renders no
italic text).
License: SIL Open Font License 1.1, full text in `OFL.txt` next to
the font.

Why bundled: Iced resolves its default sans-serif through the system
font database, which picks visibly different (and on some macOS setups
worse-rendered) faces per machine. Shipping Adwaita Sans makes body,
Medium button labels, and Bold headers render from the same outlines
on Linux and macOS. Glyphs Adwaita Sans lacks (for example CJK) still
fall back to system fonts per glyph, so nothing renders as tofu.
Loaded once at startup in `yutu_ui::run` (`iced::application::font`);
the Settings Appearance section picks the family.
