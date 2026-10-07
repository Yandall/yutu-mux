//! 10-band EQ preset types.
//!
//! Pure data + validation. No DSP here: the future mpv backend should
//! consume [`EqPreset::to_mpv_args`] (or [`EqPreset::to_mpv_af_chain`])
//! and pass the resulting `--af=...` flags to mpv.

use serde::{Deserialize, Serialize};

/// Number of EQ bands (fixed at 10, classic graphic-EQ layout).
pub const BAND_COUNT: usize = 10;
/// Minimum gain per band in dB.
pub const MIN_GAIN_DB: f32 = -12.0;
/// Maximum gain per band in dB.
pub const MAX_GAIN_DB: f32 = 12.0;

/// Center frequencies for the 10 bands (Hz).
pub const EQ_FREQUENCIES: [f32; BAND_COUNT] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EqError {
    #[error("EQ requires exactly {expected} bands, got {got}")]
    BadBandCount { expected: usize, got: usize },
    #[error("gain {gain}dB out of range (+/-12dB) at band {index}")]
    GainOutOfRange { gain: f32, index: usize },
}

/// A single EQ band: center frequency + gain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
}

impl EqBand {
    /// Create a band, validating the gain is within +/-12dB.
    pub fn new(freq_hz: f32, gain_db: f32) -> Result<Self, EqError> {
        validate_gain(gain_db, 0)?;
        Ok(Self { freq_hz, gain_db })
    }
}

/// A named 10-band EQ preset. Gains are in dB, band order matches
/// [`EQ_FREQUENCIES`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub bands: [f32; BAND_COUNT],
}

fn validate_gain(gain: f32, index: usize) -> Result<(), EqError> {
    if !gain.is_finite() || gain < MIN_GAIN_DB || gain > MAX_GAIN_DB {
        return Err(EqError::GainOutOfRange { gain, index });
    }
    Ok(())
}

/// Validate a gain slice: must be exactly 10 bands, each within +/-12dB.
pub fn validate_gains(bands: &[f32]) -> Result<(), EqError> {
    if bands.len() != BAND_COUNT {
        return Err(EqError::BadBandCount {
            expected: BAND_COUNT,
            got: bands.len(),
        });
    }
    for (i, &g) in bands.iter().enumerate() {
        validate_gain(g, i)?;
    }
    Ok(())
}

impl EqPreset {
    /// Build a preset from a name + gain slice (must be exactly 10 bands).
    pub fn new(name: impl Into<String>, bands: &[f32]) -> Result<Self, EqError> {
        validate_gains(bands)?;
        let mut arr = [0.0f32; BAND_COUNT];
        arr.copy_from_slice(bands);
        Ok(Self {
            name: name.into(),
            bands: arr,
        })
    }

    /// Re-validate `self` (useful after deserialization).
    pub fn validate(&self) -> Result<(), EqError> {
        validate_gains(&self.bands)
    }

    /// Bands zipped with their center frequencies.
    #[must_use]
    pub fn as_bands(&self) -> Vec<EqBand> {
        EQ_FREQUENCIES
            .iter()
            .zip(self.bands.iter())
            .map(|(&freq_hz, &gain_db)| EqBand { freq_hz, gain_db })
            .collect()
    }

    /// One `--af=equalizer=...` flag per band for the future mpv backend.
    ///
    /// Each entry uses the lavfi `equalizer` syntax:
    /// `--af=equalizer=f={freq}:t=h:w=1:g={gain}`.
    #[must_use]
    pub fn to_mpv_args(&self) -> Vec<String> {
        EQ_FREQUENCIES
            .iter()
            .zip(self.bands.iter())
            .map(|(freq, gain)| format!("--af=equalizer=f={freq}:t=h:w=1:g={gain}"))
            .collect()
    }

    /// Single comma-joined filter chain for one `--af` flag, e.g.
    /// `equalizer=f=31:... ,equalizer=f=62:...`.
    #[must_use]
    pub fn to_mpv_af_chain(&self) -> String {
        EQ_FREQUENCIES
            .iter()
            .zip(self.bands.iter())
            .map(|(freq, gain)| format!("equalizer=f={freq}:t=h:w=1:g={gain}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[must_use]
    pub fn flat() -> Self {
        Self {
            name: "Flat".to_string(),
            bands: [0.0; BAND_COUNT],
        }
    }

    #[must_use]
    pub fn bass_boost() -> Self {
        Self {
            name: "Bass Boost".to_string(),
            bands: [6.0, 5.0, 4.0, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        }
    }

    #[must_use]
    pub fn treble_boost() -> Self {
        Self {
            name: "Treble Boost".to_string(),
            bands: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        }
    }

    #[must_use]
    pub fn vocal() -> Self {
        Self {
            name: "Vocal".to_string(),
            bands: [-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 1.0, 0.0, -1.0],
        }
    }

    #[must_use]
    pub fn rock() -> Self {
        Self {
            name: "Rock".to_string(),
            bands: [5.0, 4.0, 3.0, 2.0, -1.0, -1.0, 2.0, 3.0, 4.0, 5.0],
        }
    }

    #[must_use]
    pub fn pop() -> Self {
        Self {
            name: "Pop".to_string(),
            bands: [-1.0, 0.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0, -1.0],
        }
    }

    #[must_use]
    pub fn jazz() -> Self {
        Self {
            name: "Jazz".to_string(),
            bands: [4.0, 3.0, 2.0, 1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 3.0],
        }
    }

    #[must_use]
    pub fn classical() -> Self {
        Self {
            name: "Classical".to_string(),
            bands: [4.0, 3.0, 2.0, 1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0],
        }
    }

    /// All built-in presets in a stable order.
    #[must_use]
    pub fn builtin_presets() -> Vec<Self> {
        vec![
            Self::flat(),
            Self::bass_boost(),
            Self::treble_boost(),
            Self::vocal(),
            Self::rock(),
            Self::pop(),
            Self::jazz(),
            Self::classical(),
        ]
    }

    /// Look up a built-in preset by name (case-insensitive).
    #[must_use]
    pub fn preset_by_name(name: &str) -> Option<Self> {
        Self::builtin_presets()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_band_counts() {
        assert!(matches!(
            EqPreset::new("x", &[0.0; 9]),
            Err(EqError::BadBandCount { got: 9, .. })
        ));
        assert!(matches!(
            EqPreset::new("x", &[0.0; 11]),
            Err(EqError::BadBandCount { got: 11, .. })
        ));
        assert!(matches!(
            validate_gains(&[]),
            Err(EqError::BadBandCount { .. })
        ));
    }

    #[test]
    fn rejects_out_of_range_gains() {
        let mut bands = [0.0f32; BAND_COUNT];
        bands[3] = 12.1;
        assert!(matches!(
            EqPreset::new("x", &bands),
            Err(EqError::GainOutOfRange { .. })
        ));
        bands[3] = -12.1;
        assert!(matches!(
            EqPreset::new("x", &bands),
            Err(EqError::GainOutOfRange { .. })
        ));
        bands[3] = f32::NAN;
        assert!(EqPreset::new("x", &bands).is_err());
        // Boundaries are inclusive.
        bands[3] = 12.0;
        assert!(EqPreset::new("x", &bands).is_ok());
        bands[3] = -12.0;
        assert!(EqPreset::new("x", &bands).is_ok());
    }

    #[test]
    fn all_builtin_presets_are_valid() {
        let presets = EqPreset::builtin_presets();
        assert_eq!(presets.len(), 8);
        for p in &presets {
            assert!(p.validate().is_ok(), "preset {} invalid", p.name);
            assert_eq!(p.bands.len(), BAND_COUNT);
        }
        // Spot-check names.
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        for want in [
            "Flat",
            "Bass Boost",
            "Treble Boost",
            "Vocal",
            "Rock",
            "Pop",
            "Jazz",
            "Classical",
        ] {
            assert!(names.contains(&want), "missing preset {want}");
        }
    }

    #[test]
    fn mpv_args_format() {
        let p = EqPreset::flat();
        let args = p.to_mpv_args();
        assert_eq!(args.len(), BAND_COUNT);
        for (i, arg) in args.iter().enumerate() {
            let freq = EQ_FREQUENCIES[i];
            assert!(arg.starts_with("--af=equalizer=f="), "bad prefix: {arg}");
            assert!(arg.contains(&format!("f={freq}:")), "bad freq: {arg}");
            assert!(arg.contains(":t=h:w=1:g="), "bad filter shape: {arg}");
        }
        assert_eq!(args[0], "--af=equalizer=f=31:t=h:w=1:g=0");
        // Chain form joins the same filters with commas.
        let chain = p.to_mpv_af_chain();
        assert_eq!(chain.split(',').count(), BAND_COUNT);
        assert!(chain.starts_with("equalizer=f=31:"));
    }

    #[test]
    fn serde_roundtrip() {
        let p = EqPreset::rock();
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EqPreset = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn preset_by_name_is_case_insensitive() {
        assert_eq!(EqPreset::preset_by_name("flat").unwrap().name, "Flat");
        assert_eq!(EqPreset::preset_by_name("ROCK").unwrap().name, "Rock");
        assert!(EqPreset::preset_by_name("nope").is_none());
    }
}
