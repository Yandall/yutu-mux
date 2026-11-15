//! Favorites cover: a white heart on a two-color gradient.
//!
//! The gradient is picked at random once (two harmonious vivid hues and an
//! angle) and saved in the `favorites_cover` pref, so it never changes
//! after the first launch. The cover is rasterized in memory from that
//! spec (pure: same spec, same pixels), and its page glow is the same
//! 8x8 Triangle downscale every image cover gets.

use iced::widget::image::Handle;

/// Pref holding the saved gradient (`#rrggbb,#rrggbb,<degrees>`).
pub(crate) const FAVORITES_COVER_PREF: &str = "favorites_cover";
/// Raster edge: crisp up to the 190px header on 2x displays.
pub(crate) const COVER_PX: u32 = 384;

/// The saved look: gradient end colors and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoverSpec {
    pub(crate) from: [u8; 3],
    pub(crate) to: [u8; 3],
    /// Gradient direction in degrees (0 = left to right, 90 = top down).
    pub(crate) angle: u16,
}

impl CoverSpec {
    /// Stored form: `#rrggbb,#rrggbb,<degrees>`.
    pub(crate) fn to_pref(self) -> String {
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        format!("{},{},{}", hex(self.from), hex(self.to), self.angle)
    }

    /// Parse the stored form; `None` on anything malformed.
    pub(crate) fn from_pref(raw: &str) -> Option<Self> {
        let mut parts = raw.trim().split(',');
        let color = |s: &str| -> Option<[u8; 3]> {
            let h = s.trim().strip_prefix('#')?;
            if h.len() != 6 {
                return None;
            }
            let v = u32::from_str_radix(h, 16).ok()?;
            Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
        };
        let from = color(parts.next()?)?;
        let to = color(parts.next()?)?;
        let angle = parts.next()?.trim().parse::<u16>().ok()? % 360;
        parts.next().is_none().then_some(Self { from, to, angle })
    }

    /// A fresh random look: a vivid hue and a neighbour 30-100 degrees
    /// around the wheel (always harmonious, never muddy), random angle.
    pub(crate) fn random() -> Self {
        let mut rng = Rng::seeded();
        let h1 = rng.next() % 360;
        let h2 = (h1 + 30 + rng.next() % 71) % 360;
        Self {
            from: hsl(h1 as f32, 0.72, 0.52),
            to: hsl(h2 as f32, 0.78, 0.38),
            angle: (rng.next() % 360) as u16,
        }
    }
}

/// The cover image: gradient plus a centered white heart, `COVER_PX`
/// square RGBA. The gradient runs edge to edge along `angle` (its end
/// colors sit on the two extreme corners/edges). The heart edge is
/// antialiased: pixel corners are tested once (shared with neighbours)
/// and only pixels the edge crosses take 4 subsamples.
pub(crate) fn render(spec: CoverSpec) -> Vec<u8> {
    let n = COVER_PX as usize;
    let mut out = vec![0u8; n * n * 4];
    let (sin, cos) = (spec.angle as f32).to_radians().sin_cos();
    // Half the square's extent along the gradient direction.
    let half = 0.5 * (cos.abs() + sin.abs());
    // Heart test at every pixel corner, once.
    let corners: Vec<bool> = (0..=n)
        .flat_map(|cy| (0..=n).map(move |cx| (cx, cy)))
        .map(|(cx, cy)| in_heart(cx as f32 / n as f32, cy as f32 / n as f32))
        .collect();
    let corner = |cx: usize, cy: usize| corners[cy * (n + 1) + cx];
    for py in 0..n {
        for px in 0..n {
            let x = px as f32 / (n - 1) as f32 - 0.5;
            let y = py as f32 / (n - 1) as f32 - 0.5;
            let t = ((x * cos + y * sin) / (2.0 * half) + 0.5).clamp(0.0, 1.0);
            let bg = [0, 1, 2].map(|i| lerp(spec.from[i], spec.to[i], t));
            let quad = [
                corner(px, py),
                corner(px + 1, py),
                corner(px, py + 1),
                corner(px + 1, py + 1),
            ];
            let cover = if quad.iter().all(|&c| c) {
                1.0
            } else if quad.iter().all(|&c| !c) {
                0.0
            } else {
                [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                    .iter()
                    .filter(|(sx, sy)| {
                        in_heart((px as f32 + sx) / n as f32, (py as f32 + sy) / n as f32)
                    })
                    .count() as f32
                    / 4.0
            };
            let i = (py * n + px) * 4;
            for c in 0..3 {
                out[i + c] = (bg[c] * (1.0 - cover) + 255.0 * cover).round() as u8;
            }
            out[i + 3] = 255;
        }
    }
    out
}

/// The page glow: the cover shrunk to `px` x `px` with the Triangle
/// filter, exactly like every downloaded cover's glow.
pub(crate) fn blur(rgba: &[u8], px: u32) -> Vec<u8> {
    match image::RgbaImage::from_raw(COVER_PX, COVER_PX, rgba.to_vec()) {
        Some(img) => {
            image::imageops::resize(&img, px, px, image::imageops::FilterType::Triangle).into_raw()
        }
        None => vec![0; (px * px * 4) as usize],
    }
}

/// Decoded cover and glow handles, built once at launch.
#[derive(Debug, Clone)]
pub(crate) struct FavoritesCover {
    pub(crate) image: Handle,
    pub(crate) glow: Handle,
}

impl FavoritesCover {
    pub(crate) fn build(spec: CoverSpec, glow_px: u32) -> Self {
        let rgba = render(spec);
        let glow = blur(&rgba, glow_px);
        Self {
            image: Handle::from_rgba(COVER_PX, COVER_PX, rgba),
            glow: Handle::from_rgba(glow_px, glow_px, glow),
        }
    }
}

/// The saved look from `store`, or a new random one saved on the spot
/// (first launch, or a damaged pref).
pub(crate) fn load_or_create(store: &yutu_core::LibraryStore) -> CoverSpec {
    if let Some(spec) = store
        .get_pref(FAVORITES_COVER_PREF)
        .ok()
        .flatten()
        .and_then(|raw| CoverSpec::from_pref(&raw))
    {
        return spec;
    }
    let spec = CoverSpec::random();
    let _ = store.set_pref(FAVORITES_COVER_PREF, &spec.to_pref());
    spec
}

/// Heart test in unit coordinates (0..1, y down): the classic implicit
/// heart `(x^2 + y^2 - 1)^3 - x^2 y^3 <= 0` (about 2.3 units wide),
/// scaled to ~45% of the edge so the gradient carries the cover and its
/// glow, nudged down a little to sit optically centered.
fn in_heart(u: f32, v: f32) -> bool {
    const SCALE: f32 = 0.195;
    let x = (u - 0.5) / SCALE;
    let y = -(v - 0.515) / SCALE;
    let a = x * x + y * y - 1.0;
    a * a * a - x * x * y * y * y <= 0.0
}

fn lerp(a: u8, b: u8, t: f32) -> f32 {
    a as f32 + (b as f32 - a as f32) * t
}

/// HSL to RGB (`h` in degrees, `s`/`l` 0..1).
fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Tiny xorshift, seeded from the clock and the process's random hash
/// keys (no `rand` dependency for one pick per install).
struct Rng(u64);

impl Rng {
    fn seeded() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        Self(h.finish() | 1)
    }

    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_round_trips_through_the_pref() {
        let spec = CoverSpec {
            from: [0xff, 0x00, 0x33],
            to: [0x12, 0x34, 0x56],
            angle: 135,
        };
        assert_eq!(spec.to_pref(), "#ff0033,#123456,135");
        assert_eq!(CoverSpec::from_pref(&spec.to_pref()), Some(spec));
        for bad in [
            "",
            "#ff0033",
            "#ff0033,#123456",
            "ff0033,#123456,1",
            "#ff00,#123456,1",
            "#ff0033,#123456,x",
            "#ff0033,#123456,1,2",
        ] {
            assert_eq!(CoverSpec::from_pref(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn random_looks_are_vivid_and_valid() {
        for _ in 0..50 {
            let spec = CoverSpec::random();
            assert!(spec.angle < 360);
            assert_ne!(spec.from, spec.to);
            // Saturated enough to read as color, not grey.
            let spread = |c: [u8; 3]| c.iter().max().unwrap() - c.iter().min().unwrap();
            assert!(spread(spec.from) > 80 && spread(spec.to) > 60, "{spec:?}");
            assert_eq!(CoverSpec::from_pref(&spec.to_pref()), Some(spec));
        }
    }

    #[test]
    fn render_is_pure_and_draws_a_white_heart_on_the_gradient() {
        let spec = CoverSpec {
            from: [200, 30, 60],
            to: [40, 20, 160],
            angle: 0,
        };
        let a = render(spec);
        assert_eq!(a, render(spec));
        assert_eq!(a.len(), (COVER_PX * COVER_PX * 4) as usize);
        let px = |x: u32, y: u32| {
            let i = ((y * COVER_PX + x) * 4) as usize;
            [a[i], a[i + 1], a[i + 2], a[i + 3]]
        };
        let n = COVER_PX - 1;
        // Center is the heart (white); corners are the gradient ends.
        assert_eq!(px(n / 2, n / 2), [255, 255, 255, 255]);
        assert_eq!(px(0, 0)[..3], [200, 30, 60]);
        assert_eq!(px(n, 0)[..3], [40, 20, 160]);
    }

    #[test]
    fn glow_is_the_8px_downscale_with_the_gradient_colors() {
        let spec = CoverSpec {
            from: [220, 20, 40],
            to: [20, 40, 220],
            angle: 0,
        };
        let g = blur(&render(spec), 8);
        assert_eq!(g.len(), 8 * 8 * 4);
        // Left edge leans red, right edge leans blue.
        let left = &g[(3 * 8) * 4..(3 * 8) * 4 + 3];
        let right = &g[(3 * 8 + 7) * 4..(3 * 8 + 7) * 4 + 3];
        assert!(
            left[0] > left[2] && right[2] > right[0],
            "{left:?} {right:?}"
        );
    }

    #[test]
    fn the_look_is_created_once_then_kept() {
        let path = std::env::temp_dir().join(format!("yutu-favcover-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = yutu_core::LibraryStore::open(path.to_str().unwrap()).unwrap();
        let first = load_or_create(&store);
        for _ in 0..3 {
            assert_eq!(load_or_create(&store), first);
        }
        // A damaged pref is replaced (and then kept).
        store.set_pref(FAVORITES_COVER_PREF, "garbage").unwrap();
        let fresh = load_or_create(&store);
        assert_eq!(load_or_create(&store), fresh);
        let _ = std::fs::remove_file(&path);
    }
}
