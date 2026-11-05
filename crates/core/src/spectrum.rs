//! The spectrum of what is playing, for drawing: [`BANDS`] levels from 0 to 1, low pitch to high.
//!
//! The audio output feeds the samples it plays ([`feed`], from the audio thread: lock-free), and
//! the interface asks for the levels as often as it draws ([`bands`]). Nothing here knows about
//! any audio library. The levels are analysed on the reading side, from the most recent samples,
//! and they rise quickly and fall slowly, so the bars read as music. When nothing is fed (paused,
//! stopped) they fall to zero.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// How many bands the spectrum has.
pub const BANDS: usize = 32;
/// The frames analysed at a time (about 43 ms at 48 kHz).
const WINDOW: usize = 2048;
/// Capacity of the sample ring: a window of up to four channels.
const RING: usize = WINDOW * 4;
const LOWEST_HZ: f32 = 60.0;
const HIGHEST_HZ: f32 = 9_000.0;
/// Without fresh samples for this long, the sound has stopped.
const STALE_MS: u64 = 120;
/// How far a band rises towards a louder level per reading, and how much of its height it keeps
/// when the level is lower.
const RISE: f32 = 0.7;
const FALL: f32 = 0.85;
/// The levels (in dB below full scale) that map to 0 and to 1.
const FLOOR_DB: f32 = -60.0;
const CEILING_DB: f32 = -12.0;

static SAMPLES: [AtomicU32; RING] = [const { AtomicU32::new(0) }; RING];
/// How many samples have ever been fed.
static HEAD: AtomicUsize = AtomicUsize::new(0);
static RATE: AtomicU32 = AtomicU32::new(48_000);
static CHANNELS: AtomicU32 = AtomicU32::new(2);
/// When samples were last fed, in milliseconds since [`start`].
static LAST_MS: AtomicU64 = AtomicU64::new(u64::MAX);
static LEVELS: Mutex<[f32; BANDS]> = Mutex::new([0.0; BANDS]);

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    start().elapsed().as_millis() as u64
}

/// Hands over interleaved `samples` the output is playing, at `rate` Hz with `channels`. Called
/// from the audio thread: it takes no lock and does not block.
pub fn feed(rate: u32, channels: u16, samples: &[f32]) {
    if samples.is_empty() || channels == 0 {
        return;
    }
    RATE.store(rate.max(1), Ordering::Relaxed);
    CHANNELS.store(u32::from(channels), Ordering::Relaxed);
    let head = HEAD.load(Ordering::Relaxed);
    for (i, s) in samples.iter().enumerate() {
        SAMPLES[(head + i) % RING].store(s.to_bits(), Ordering::Relaxed);
    }
    HEAD.store(head + samples.len(), Ordering::Release);
    LAST_MS.store(now_ms(), Ordering::Relaxed);
}

/// The centre frequency of band `k`: spaced evenly on a log scale.
fn centre_hz(k: usize) -> f32 {
    LOWEST_HZ * (HIGHEST_HZ / LOWEST_HZ).powf(k as f32 / (BANDS - 1) as f32)
}

/// The latest window as mono, Hann-weighted, or `None` when fewer samples have been fed.
fn window() -> Option<(Vec<f32>, f32)> {
    let ch = CHANNELS.load(Ordering::Relaxed).max(1) as usize;
    let head = HEAD.load(Ordering::Acquire);
    let need = WINDOW * ch;
    if head < need {
        return None;
    }
    let first = head - need;
    let mut mono = Vec::with_capacity(WINDOW);
    for f in 0..WINDOW {
        let mut sum = 0.0;
        for c in 0..ch {
            sum += f32::from_bits(SAMPLES[(first + f * ch + c) % RING].load(Ordering::Relaxed));
        }
        let hann = 0.5 - 0.5 * (std::f32::consts::TAU * f as f32 / (WINDOW - 1) as f32).cos();
        mono.push(sum / ch as f32 * hann);
    }
    Some((mono, RATE.load(Ordering::Relaxed) as f32))
}

/// The amplitude (0 to about 1 for a full-scale sine) of `hz` in `x`, by the Goertzel recurrence.
fn amplitude(x: &[f32], rate: f32, hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz / rate;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for v in x {
        let s0 = v + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    // A Hann window passes half of a sine's amplitude; a window of N frames sums to N/2 of it.
    power.max(0.0).sqrt() / (x.len() as f32 * 0.25)
}

fn level_of(amp: f32) -> f32 {
    let db = 20.0 * amp.max(1e-6).log10();
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// The levels now, from 0 to 1, smoothed over the readings before. Falls to zero when nothing is
/// being fed.
pub fn bands() -> [f32; BANDS] {
    let last = LAST_MS.load(Ordering::Relaxed);
    let fresh = last != u64::MAX && now_ms().saturating_sub(last) <= STALE_MS;
    let target: [f32; BANDS] = match fresh.then(window).flatten() {
        Some((x, rate)) => {
            let mut t = [0.0; BANDS];
            for (k, level) in t.iter_mut().enumerate() {
                let hz = centre_hz(k);
                if hz < rate / 2.0 {
                    *level = level_of(amplitude(&x, rate, hz));
                }
            }
            t
        }
        None => [0.0; BANDS],
    };
    let mut levels = LEVELS.lock().unwrap_or_else(|e| e.into_inner());
    for (l, t) in levels.iter_mut().zip(target) {
        *l = if t > *l {
            *l + (t - *l) * RISE
        } else {
            *l * FALL
        };
        if *l < 1e-3 {
            *l = 0.0;
        }
    }
    *levels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test owns the global state: feeding, reading and going quiet, in order.
    #[test]
    fn a_tone_lights_its_band_and_silence_lets_it_fall() {
        let rate = 48_000u32;
        // A tone at a band's own centre, so the whole of it lands in that band.
        let band = 14;
        let hz = centre_hz(band);
        let tone: Vec<f32> = (0..WINDOW * 2 * 2)
            .map(|i| {
                let frame = (i / 2) as f32;
                0.5 * (std::f32::consts::TAU * hz * frame / rate as f32).sin()
            })
            .collect();
        feed(rate, 2, &tone);
        let mut b = bands();
        for _ in 0..8 {
            b = bands(); // the bars rise over a few readings
        }
        let peak = (0..BANDS)
            .max_by(|&x, &y| b[x].total_cmp(&b[y]))
            .expect("bands");
        assert_eq!(peak, band, "the tone's own band is the loudest: {b:?}");
        assert!(b[peak] > 0.5, "a half-scale tone is loud: {}", b[peak]);
        assert!(b.iter().all(|l| (0.0..=1.0).contains(l)));
        // Nothing fed any more: after the stale time the bands fall to nothing.
        std::thread::sleep(std::time::Duration::from_millis(STALE_MS + 60));
        for _ in 0..80 {
            b = bands();
        }
        assert!(b.iter().all(|l| *l == 0.0), "fell to zero: {b:?}");
    }

    #[test]
    fn bands_are_spaced_from_low_to_high() {
        assert!((centre_hz(0) - LOWEST_HZ).abs() < 0.01);
        assert!((centre_hz(BANDS - 1) - HIGHEST_HZ).abs() < 1.0);
        assert!((1..BANDS).all(|k| centre_hz(k) > centre_hz(k - 1)));
    }
}
