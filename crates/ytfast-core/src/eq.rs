//! A ten-band equalizer for what YTFast plays (Settings, Equalizer).
//!
//! Adapted from Spotifast's `src/eq.rs` (MIT, Copyright (c) 2026 Carmine
//! Paolino): Winamp's bands, a peaking filter for each (after the Audio EQ
//! Cookbook), their gains solved together so the sound meets each slider at
//! its band's centre, and its presets. YTFast's own: the filters run at the
//! device's rate, on the device's thread (`audio::MixerRender`), for any
//! number of channels; and the level comes down by the curve's highest
//! boost, so a boost never pushes the loudest songs past full scale (no
//! preamp to set). No balance or mono.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// The centre frequencies, Winamp's, in hertz.
pub const BANDS: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
/// How far a band goes either way, in decibels.
pub const RANGE_DB: f32 = 12.0;
/// Bands closer to flat than this are left out rather than run for nothing.
const FLAT: f32 = 0.05;
/// How far the solved gains may go past the sliders while making the
/// combined response meet them; a guard, not a target.
const SOLVED_LIMIT: f64 = 36.0;
/// The highest frequency a band is run at, as a share of the sample rate:
/// a peaking filter at the rate's top would do nothing useful.
const TOP_OF_RATE: f64 = 0.45;

/// The equalizer's settings: on or off, and each band's gain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Equalizer {
    pub on: bool,
    /// Each band's gain, [`RANGE_DB`] either way, in [`BANDS`]' order.
    pub bands_db: [f32; 10],
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            on: false,
            bands_db: [0.0; 10],
        }
    }
}

impl Equalizer {
    /// The same settings kept within what the equalizer can do.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        for band in &mut self.bands_db {
            *band = if band.is_finite() {
                band.clamp(-RANGE_DB, RANGE_DB)
            } else {
                0.0
            };
        }
        self
    }

    /// Every band at 0 dB: the sound as it is.
    pub fn is_flat(&self) -> bool {
        self.bands_db.iter().all(|band| band.abs() <= FLAT)
    }

    /// The preset these settings are, if any.
    pub fn preset(&self) -> Option<&'static Preset> {
        PRESETS.iter().find(|preset| {
            preset
                .bands_db
                .iter()
                .zip(self.bands_db)
                .all(|(a, b)| (a - b).abs() < 0.05)
        })
    }

    /// What is played at `rate` samples a second: the filters, and the
    /// level they come down by.
    pub fn curve(&self, rate: u32) -> Curve {
        let filters = chain(&self.bands_db, f64::from(rate));
        let peak = loudest_db(&filters, f64::from(rate));
        Curve {
            filters,
            level_db: -peak.max(0.0),
        }
    }
}

/// What the equalizer plays: its filters, and the level they come down by
/// so the loudest boost stays at full scale.
pub struct Curve {
    filters: Vec<Biquad>,
    level_db: f64,
}

impl Curve {
    /// The response at `hz`, in decibels, the level's lowering included.
    pub fn db_at(&self, hz: f32) -> f32 {
        let shape: f64 = self
            .filters
            .iter()
            .map(|filter| filter.gain_db_at(f64::from(hz)))
            .sum();
        (shape + self.level_db) as f32
    }

    /// How far the level comes down, in decibels (0 or less).
    pub fn level_db(&self) -> f32 {
        self.level_db as f32
    }
}

/// The highest point of the filters' response, in decibels, across what is
/// heard (20 Hz up to the rate's top).
fn loudest_db(filters: &[Biquad], rate: f64) -> f64 {
    if filters.is_empty() {
        return 0.0;
    }
    let (low, high) = (20f64.ln(), (20_000f64).min(rate * TOP_OF_RATE).ln());
    (0..=240)
        .map(|step| (low + (high - low) * f64::from(step) / 240.0).exp())
        .map(|hz| {
            filters
                .iter()
                .map(|f| f.gain_db_at_rate(hz, rate))
                .sum::<f64>()
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Band widths in octaves, based on the midpoint between adjacent bands.
///
/// The bands are unevenly spaced. A shared width makes the high bands overlap
/// and overboost presets such as Full Treble. The outer bands extend three
/// octaves below 60 Hz and one octave above 16 kHz, matching Winamp.
fn band_widths() -> [f64; 10] {
    let octaves: Vec<f64> = BANDS.iter().map(|hz| f64::from(*hz).log2()).collect();
    let mut widths = [0.0; 10];
    for (index, width) in widths.iter_mut().enumerate() {
        let below = index
            .checked_sub(1)
            .map_or(octaves[index] - 3.0, |i| octaves[i]);
        let above = octaves
            .get(index + 1)
            .copied()
            .unwrap_or(octaves[index] + 1.0);
        *width = (octaves[index] - below) / 2.0 + (above - octaves[index]) / 2.0;
    }
    widths
}

/// Builds filters whose combined response matches each slider at its centre.
/// Adjacent filters overlap, so their individual gains are solved together.
fn chain(bands_db: &[f32; 10], rate: f64) -> Vec<Biquad> {
    let widths = band_widths();
    let target: [f64; 10] = bands_db.map(f64::from);
    if target.iter().all(|gain| gain.abs() <= f64::from(FLAT)) {
        return Vec::new();
    }
    // How much each band moves every centre, per decibel it is given.
    let mut unit = [[0.0; 10]; 10];
    for (j, (hz, width)) in BANDS.iter().zip(widths).enumerate() {
        let filter = Biquad::peaking(f64::from(*hz), width, 1.0, rate);
        for (i, centre) in BANDS.iter().enumerate() {
            unit[i][j] = filter.gain_db_at(f64::from(*centre));
        }
    }
    let mut gains = target;
    for _ in 0..6 {
        let filters = filters_for(&gains, &widths, rate);
        let mut residual = [0.0; 10];
        for (i, centre) in BANDS.iter().enumerate() {
            let played: f64 = filters
                .iter()
                .map(|filter| filter.gain_db_at(f64::from(*centre)))
                .sum();
            residual[i] = played - target[i];
        }
        if residual.iter().all(|r| r.abs() < 0.01) {
            break;
        }
        let correction = solve(unit, residual);
        for (gain, step) in gains.iter_mut().zip(correction) {
            *gain = (*gain - step).clamp(-SOLVED_LIMIT, SOLVED_LIMIT);
        }
    }
    filters_for(&gains, &widths, rate)
}

/// The bands worth running at `rate`, with the filter for each.
fn filters_for(gains: &[f64; 10], widths: &[f64; 10], rate: f64) -> Vec<Biquad> {
    BANDS
        .iter()
        .zip(widths)
        .zip(gains)
        .filter(|((hz, _), gain)| {
            gain.abs() > f64::from(FLAT) && f64::from(**hz) < rate * TOP_OF_RATE
        })
        .map(|((hz, width), gain)| Biquad::peaking(f64::from(*hz), *width, *gain, rate))
        .collect()
}

/// Gaussian elimination with partial pivoting, for the ten-by-ten
/// interaction of the bands.
fn solve(mut matrix: [[f64; 10]; 10], mut rhs: [f64; 10]) -> [f64; 10] {
    let n = 10;
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|a, b| {
                matrix[*a][column]
                    .abs()
                    .total_cmp(&matrix[*b][column].abs())
            })
            .unwrap_or(column);
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let lead = matrix[column][column];
        if lead.abs() < 1e-12 {
            continue;
        }
        let pivot_row = matrix[column];
        let pivot_rhs = rhs[column];
        for row in column + 1..n {
            let factor = matrix[row][column] / lead;
            if factor == 0.0 {
                continue;
            }
            for (cell, above) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *cell -= factor * above;
            }
            rhs[row] -= factor * pivot_rhs;
        }
    }
    let mut solution = [0.0; 10];
    for row in (0..n).rev() {
        let mut sum = rhs[row];
        for k in row + 1..n {
            sum -= matrix[row][k] * solution[k];
        }
        let lead = matrix[row][row];
        solution[row] = if lead.abs() < 1e-12 { 0.0 } else { sum / lead };
    }
    solution
}

/// A named set of band gains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub bands_db: [f32; 10],
}

/// The presets offered, Flat first: Spotifast's values (Winamp's own, and
/// its scenario presets), a few left out to keep the list short.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Flat",
        bands_db: [0.0; 10],
    },
    Preset {
        name: "Bass Booster",
        bands_db: [8.8, 7.2, 5.6, 3.2, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Bass Reducer",
        bands_db: [-8.8, -7.2, -5.6, -3.2, -0.8, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Treble Booster",
        bands_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 3.2, 5.6, 7.2, 8.8],
    },
    Preset {
        name: "Vocal Booster",
        bands_db: [-2.4, -4.8, -4.8, 1.6, 5.6, 5.6, 4.0, 1.6, 0.0, -2.4],
    },
    Preset {
        name: "Loudness",
        bands_db: [9.6, 6.4, 0.0, 0.0, -2.4, 0.0, -1.6, 0.0, 8.0, 1.6],
    },
    Preset {
        name: "Headphones",
        bands_db: [4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12.0, 12.0],
    },
    Preset {
        name: "Small Speakers",
        bands_db: [-8.0, -6.4, -4.0, -1.6, 1.6, 3.2, 4.8, 5.6, 5.6, 5.6],
    },
    Preset {
        name: "Night Listening",
        bands_db: [-4.8, -3.2, -1.6, 0.8, 2.4, 3.2, 2.4, 0.8, -1.6, -3.2],
    },
    Preset {
        name: "Spoken Word",
        bands_db: [-3.2, -0.8, 0.0, 0.8, 4.0, 5.6, 4.8, 2.4, 0.8, 0.0],
    },
    Preset {
        name: "Pop",
        bands_db: [-1.6, 4.8, 7.2, 8.0, 5.6, 0.0, -2.4, -2.4, -1.6, -1.6],
    },
    Preset {
        name: "Rock",
        bands_db: [8.0, 4.8, -5.6, -8.0, -3.2, 4.0, 8.8, 11.2, 11.2, 11.2],
    },
    Preset {
        name: "Dance",
        bands_db: [9.6, 7.2, 2.4, 0.0, 0.0, -5.6, -7.2, -7.2, 0.0, 0.0],
    },
    Preset {
        name: "Classical",
        bands_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.2, -7.2, -7.2, -9.6],
    },
];

/// The settings as the window and the device's thread share them: the
/// device's thread looks at `version` for each block of sound, and reads
/// the settings only when it changed, without ever waiting for the lock.
#[derive(Default)]
pub struct Shared {
    version: AtomicU64,
    settings: Mutex<Equalizer>,
}

impl Shared {
    /// New settings, for the sound from the next block on.
    pub fn set(&self, settings: Equalizer) {
        *self.settings.lock().unwrap_or_else(PoisonError::into_inner) = settings.clamped();
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn get(&self) -> Equalizer {
        *self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A second-order section in direct form I, one per band and channel.
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    rate: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// A peaking filter after the Audio EQ Cookbook, `width` octaves wide
    /// between its half-gain points and normalised by `a0`. The width is
    /// taken the cookbook's digital way, with its `w0 / sin(w0)` term: near
    /// the top of the band the plain analog Q would have come out three
    /// times too narrow, which is what rippled the treble.
    fn peaking(hz: f64, width: f64, gain_db: f64, rate: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = std::f64::consts::TAU * hz / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin * ((std::f64::consts::LN_2 / 2.0) * width * w0 / sin).sinh();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
            rate,
            ..Self::default()
        }
    }

    /// The filter's gain at a frequency, in decibels, from its transfer
    /// function on the unit circle: what is played, including the bend
    /// the bilinear transform puts near the top of the band.
    fn gain_db_at(&self, hz: f64) -> f64 {
        self.gain_db_at_rate(hz, self.rate)
    }

    fn gain_db_at_rate(&self, hz: f64, rate: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / rate;
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let numerator = (self.b0 + self.b1 * cos1 + self.b2 * cos2).powi(2)
            + (self.b1 * sin1 + self.b2 * sin2).powi(2);
        let denominator = (1.0 + self.a1 * cos1 + self.a2 * cos2).powi(2)
            + (self.a1 * sin1 + self.a2 * sin2).powi(2);
        10.0 * (numerator / denominator).log10()
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// The filters on the device's thread, made for its rate and channels, and
/// made again when the settings change.
pub struct Processor {
    shared: Arc<Shared>,
    /// The settings' version last read (`u64::MAX`: none yet).
    seen: u64,
    applied: Equalizer,
    rate: u32,
    channels: usize,
    /// One chain per channel; empty while off or flat.
    chains: Vec<Vec<Biquad>>,
    /// The level, as a factor.
    gain: f64,
}

impl Processor {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            seen: u64::MAX,
            applied: Equalizer::default(),
            rate: 44_100,
            channels: 2,
            chains: Vec::new(),
            gain: 1.0,
        }
    }

    /// The device's format: the filters are made for it.
    pub fn configure(&mut self, rate: u32, channels: u16) {
        self.rate = rate.max(1);
        self.channels = usize::from(channels.max(1));
        self.rebuild();
    }

    fn rebuild(&mut self) {
        if !self.applied.on || self.applied.is_flat() {
            self.chains = Vec::new();
            self.gain = 1.0;
            return;
        }
        let curve = self.applied.curve(self.rate);
        self.gain = 10f64.powf(curve.level_db / 20.0);
        self.chains = vec![curve.filters; self.channels];
    }

    /// Runs interleaved samples through the equalizer, in place. Never
    /// waits: settings changed meanwhile apply from the next block.
    pub fn process(&mut self, samples: &mut [f32]) {
        let version = self.shared.version.load(Ordering::Acquire);
        if version != self.seen {
            let read = self
                .shared
                .settings
                .try_lock()
                .ok()
                .map(|settings| *settings);
            if let Some(settings) = read {
                self.applied = settings;
                self.seen = version;
                self.rebuild();
            }
        }
        if self.chains.is_empty() {
            return;
        }
        for frame in samples.chunks_exact_mut(self.channels) {
            for (sample, chain) in frame.iter_mut().zip(self.chains.iter_mut()) {
                let mut y = f64::from(*sample) * self.gain;
                for filter in chain.iter_mut() {
                    y = filter.run(y);
                }
                *sample = y as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn tone(hz: f32, frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f32 / RATE as f32;
                let sample = amplitude * (std::f32::consts::TAU * hz * t).sin();
                [sample, sample]
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    fn processor(settings: Equalizer) -> Processor {
        let shared = Arc::new(Shared::default());
        shared.set(settings);
        let mut processor = Processor::new(shared);
        processor.configure(RATE, 2);
        processor
    }

    fn gain_db(processor: &mut Processor, hz: f32) -> f64 {
        let mut samples = tone(hz, 16_384, 0.25);
        let before = rms(&samples[16_384..]);
        processor.process(&mut samples);
        20.0 * (rms(&samples[16_384..]) / before).log10()
    }

    /// Off, or flat, the sound goes through untouched.
    #[test]
    fn off_or_flat_changes_nothing() {
        let original = tone(440.0, 1024, 0.5);
        let mut boosted = Equalizer {
            on: false,
            ..Equalizer::default()
        };
        boosted.bands_db[3] = 6.0;
        for settings in [
            boosted,
            Equalizer {
                on: true,
                ..Equalizer::default()
            },
        ] {
            let mut samples = original.clone();
            processor(settings).process(&mut samples);
            assert_eq!(samples, original);
        }
    }

    /// A boosted band is louder than the rest by its slider, and the level
    /// comes down so that the boost itself stays at full scale.
    #[test]
    fn a_boost_is_heard_against_the_rest_without_going_over() {
        let mut settings = Equalizer {
            on: true,
            ..Equalizer::default()
        };
        settings.bands_db[4] = 12.0; // 1 kHz
        let mut processor = processor(settings);
        let at_band = gain_db(&mut processor, 1000.0);
        let far = gain_db(&mut processor, 60.0);
        assert!(
            (at_band - far - 12.0).abs() < 1.0,
            "{at_band:.1} against {far:.1}"
        );
        assert!(at_band <= 0.1, "the boost goes over: {at_band:.1} dB");
        // A full-scale tone at the boosted band stays within full scale.
        let mut loud = tone(1000.0, 16_384, 1.0);
        processor.process(&mut loud);
        let peak = loud[8192..].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= 1.02, "peaks at {peak}");
    }

    /// A cut needs next to no lowering (the solved neighbour bumps a little
    /// between the centres): the rest plays about as it was.
    #[test]
    fn a_cut_leaves_the_level_alone() {
        let mut settings = Equalizer {
            on: true,
            ..Equalizer::default()
        };
        settings.bands_db[0] = -12.0;
        let curve = settings.curve(RATE);
        assert!(curve.level_db() > -0.5, "{}", curve.level_db());
        let mut processor = processor(settings);
        assert!(gain_db(&mut processor, 1000.0).abs() < 0.5);
        assert!(gain_db(&mut processor, 60.0) < -9.0);
    }

    /// Settings changed while playing apply from the next block.
    #[test]
    fn new_settings_apply_from_the_next_block() {
        let shared = Arc::new(Shared::default());
        let mut processor = Processor::new(Arc::clone(&shared));
        processor.configure(RATE, 2);
        let original = tone(1000.0, 1024, 0.5);
        let mut samples = original.clone();
        processor.process(&mut samples);
        assert_eq!(samples, original);
        let mut cut = Equalizer {
            on: true,
            ..Equalizer::default()
        };
        cut.bands_db[4] = -12.0;
        shared.set(cut);
        assert!(gain_db(&mut processor, 1000.0) < -9.0);
        assert_eq!(shared.get(), cut);
    }

    /// Every preset, played, meets its sliders at every band's centre
    /// (against the level it comes down by).
    #[test]
    fn what_is_played_meets_the_sliders_at_every_band() {
        for rate in [44_100, 48_000] {
            for preset in PRESETS {
                let settings = Equalizer {
                    on: true,
                    bands_db: preset.bands_db,
                };
                let curve = settings.curve(rate);
                for (hz, wanted) in BANDS.iter().zip(preset.bands_db) {
                    let got = curve.db_at(*hz) - curve.level_db();
                    assert!(
                        (got - wanted).abs() < 0.3,
                        "{} at {hz} Hz ({rate}) plays {got:.1} dB for {wanted:.1}",
                        preset.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_preset_stays_within_the_range_and_is_found_again() {
        assert_eq!(PRESETS[0].name, "Flat");
        for preset in PRESETS {
            assert!(preset.bands_db.iter().all(|band| band.abs() <= RANGE_DB));
            let settings = Equalizer {
                on: true,
                bands_db: preset.bands_db,
            };
            assert_eq!(settings.preset().map(|p| p.name), Some(preset.name));
        }
        let mut odd = Equalizer::default();
        odd.bands_db[2] = 1.0;
        assert!(odd.preset().is_none());
        odd.bands_db[2] = f32::NAN;
        odd.bands_db[3] = 40.0;
        let kept = odd.clamped();
        assert_eq!(kept.bands_db[2], 0.0);
        assert_eq!(kept.bands_db[3], RANGE_DB);
    }
}
