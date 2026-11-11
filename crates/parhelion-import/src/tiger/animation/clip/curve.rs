//! Codec 1 (`80808F72`) animated spans, the curve form most pinned stock first-person clips use.
//!
//! A track holds either a signed-short sample for every frame, or Hermite segments between
//! signed-short keys with one byte of frame delta per segment and one byte of two four-bit tangent
//! codes per segment and component, as `sundial::package_authoring::animation_curve` reads them. A
//! rotation sample stores the quaternion's `w`, `x` and `y`, and the low bit of `x` signs the
//! derived `z`. Scale and position samples lie on [-1, 1] and decode through one range and bias per
//! track. Pinned stock codec 1 spans are 4 to 25 percent of their float size, where codec 2 spans
//! are 52 to 65 percent (Dawn 1.7.2 packages, 2026-11-08).
//!
//! The encoder keeps every track within an error budget of its source frames. It greedily grows
//! each segment while a fit stays within budget, and stores a track as raw samples when that is
//! smaller. Rotations keep the sign their source frames have. A rotation key's derived `z` limits
//! its precision near `z = 0`, so keys have a budget of their own, frames between keys carry their
//! keys' errors smoothly, and a clip whose rotation error still steps between frames, or is large
//! next to how far the track turns, stays codec 2.
use super::Channel;
use crate::presentation::{append, put};
use anyhow::Result;

/// The most a decoded unit quaternion component between keys may differ from its source shifted
/// by its keys' errors, blended across the segment, about 0.06 degrees.
const ROTATION: f32 = 5e-4;
/// The most a rotation key may differ. A key derives `z` from the unit norm, so where `z` is near
/// zero and `w` near one its resolution falls to about 0.004 whatever the samples, as in stock
/// curve clips. Stock spaces such keys far apart, and so does the encoder, preferring a key within
/// `ROTATION` as a segment's end only when it costs at most half the segment.
const KEY_ROTATION: f32 = 4e-3;
/// The most a rotation's error may change from one frame to the next. First-person D1 clips whose
/// near-rest keys stepped by up to 0.0078 jittered visibly while jumping and aiming (Dawn 1.7.2,
/// 2026-11-08).
const JITTER: f32 = 1e-3;
/// The most a rotation's error may be as a share of how far the track turns, its largest
/// component range, or `FLOOR` for a track that barely turns. The budgets above are absolute, and
/// a track that turns little sits near the derived `z` limit, where they allow an error of a third
/// of the motion or more. Stock keeps such clips in codec 2: the 38-frame additive `80BC5F40`
/// every auto rifle plays while airborne turns its weapon controls 0.3 to 1 degree, and its
/// codec 1 copy missed by up to 41% of that. Copies like it shook imported weapons on screen
/// while jumping and aiming (Dawn 1.7.2, 2026-11-08). Of 481 codec 1 clips in that stage, 152
/// meet this bound.
const RELATIVE: f32 = 0.05;
/// The least a rotation's error bound falls to, about 0.01 degrees, below what aiming shows.
const FLOOR: f32 = 1e-4;
/// The most a decoded position or scale may differ from its source, besides one 16-bit step of
/// its track's range.
const LINEAR: f32 = 1e-4;
/// A segment's frame delta is one byte.
const LONGEST: usize = 255;
/// Failed segment lengths tried in a row before the longest fitting one is kept.
const PATIENCE: usize = 3;

fn width(kind: usize) -> usize {
    [1, 4, 3][kind]
}

/// Samples a frame or key stores. A rotation's `z` is derived.
fn stored(kind: usize) -> usize {
    [1, 3, 3][kind]
}

/// The range and bias that map a linear track's values onto [-1, 1], one pair per track.
fn interval(values: &[f32]) -> Option<(f32, f32)> {
    let low = values.iter().copied().fold(f32::INFINITY, f32::min);
    let high = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let range = (high - low) / 2.;
    (range.is_finite() && range > 0.).then_some((range, low + range))
}

fn budget(kind: usize, range: f32) -> f32 {
    if kind == 1 {
        ROTATION
    } else {
        LINEAR + range / 32767.
    }
}

fn key_budget(kind: usize, range: f32) -> f32 {
    if kind == 1 {
        KEY_ROTATION
    } else {
        budget(kind, range)
    }
}

/// A stored sample as the decoder reads it, before range, bias or normalization.
fn decoded(kind: usize, samples: &[i16]) -> Option<[f32; 4]> {
    let mut point = [0.; 4];
    for (value, &sample) in point.iter_mut().zip(samples) {
        *value = f32::from(sample) / 32767.;
    }
    if kind == 1 {
        let [w, x, y, _] = point;
        let stored = w * w + x * x + y * y;
        if stored > 1.001 {
            return None;
        }
        let z = (1. - stored).max(0.).sqrt();
        point = [x, y, if samples[1] & 1 == 1 { -z } else { z }, w];
    }
    Some(point)
}

fn unit(point: [f32; 4]) -> f32 {
    point.iter().map(|v| v * v).sum::<f32>().sqrt()
}

/// The decoder's segment interpolation, with the tangent codes it reads.
fn hermite(first: f32, second: f32, code: u8, t: f32) -> f32 {
    let delta = second - first;
    let tangent = |n: u8| {
        let value = (f32::from(n) - 7.) / 7.;
        delta + 0.3 * value.abs() * value
    };
    let t2 = t * t;
    let t3 = t2 * t;
    (2. * t3 - 3. * t2 + 1.) * first
        + (t3 - 2. * t2 + t) * tangent(code >> 4)
        + (-2. * t3 + 3. * t2) * second
        + (t3 - t2) * tangent(code & 15)
}

/// The tangent code whose offset from the chord is nearest `offset`.
fn nearest(offset: f64) -> usize {
    if !offset.is_finite() {
        return 7;
    }
    (0..16)
        .min_by(|&a, &b| {
            let distance = |code: usize| {
                let value = (code as f64 - 7.) / 7.;
                (0.3 * value.abs() * value - offset).abs()
            };
            distance(a).total_cmp(&distance(b))
        })
        .unwrap_or(7)
}

/// One track's source frames.
struct Source<'a> {
    kind: usize,
    values: &'a [f32],
    /// Range and bias, or `(1, 0)` for a rotation.
    interval: (f32, f32),
    budget: f32,
    key_budget: f32,
}

impl Source<'_> {
    fn frame(&self, frame: usize) -> &[f32] {
        let width = width(self.kind);
        &self.values[frame * width..(frame + 1) * width]
    }

    /// Where a stored component should lie at `frame`.
    fn goal(&self, frame: usize, component: usize) -> f32 {
        let source = self.frame(frame);
        if self.kind == 1 {
            source[component] / source.iter().map(|v| v * v).sum::<f32>().sqrt()
        } else {
            (source[component] - self.interval.1) / self.interval.0
        }
    }

    /// What a decoded point differs from `frame` by, after the decoder's range, bias or
    /// normalization, or `None` for a rotation the decoder cannot normalize.
    fn deviation(&self, point: [f32; 4], frame: usize) -> Option<[f32; 4]> {
        let source = self.frame(frame);
        let mut deviation = [0.; 4];
        if self.kind == 1 {
            let length = point.iter().map(|v| v * v).sum::<f32>().sqrt();
            let norm = source.iter().map(|v| v * v).sum::<f32>().sqrt();
            if !length.is_finite() || length < 1e-8 {
                return None;
            }
            for ((difference, value), expected) in deviation.iter_mut().zip(point).zip(source) {
                *difference = value / length - expected / norm;
            }
        } else {
            let (range, bias) = self.interval;
            for ((difference, value), expected) in deviation.iter_mut().zip(point).zip(source) {
                *difference = value * range + bias - expected;
            }
        }
        deviation.iter().all(|v| v.is_finite()).then_some(deviation)
    }

    /// How far a decoded point lies from `frame`, after the decoder's range, bias or normalization.
    fn error(&self, point: [f32; 4], frame: usize) -> f32 {
        self.deviation(point, frame)
            .map_or(f32::INFINITY, |deviation| {
                deviation.iter().fold(0f32, |error, v| error.max(v.abs()))
            })
    }

    /// The stored samples nearest `frame`, and what they decode to.
    fn key(&self, frame: usize) -> Option<([i16; 3], [f32; 4], f32)> {
        let near = |value: f32| (value * 32767.).round().clamp(-32767., 32767.) as i32;
        if self.kind != 1 {
            let mut samples = [0i16; 3];
            for (component, sample) in samples.iter_mut().enumerate().take(width(self.kind)) {
                *sample = near(self.goal(frame, component)) as i16;
            }
            let point = decoded(self.kind, &samples[..stored(self.kind)])?;
            return Some((samples, point, self.error(point, frame)));
        }
        let [x, y, z, w] = [0, 1, 2, 3].map(|component| self.goal(frame, component));
        let center = [near(w), near(x), near(y)];
        // x carries z's sign in its low bit. Near z = 0 either sign fits, and a stored norm just
        // over 1 decodes z as 0.
        let parities: &[i32] = if z.abs() < 0.02 {
            &[0, 1]
        } else if z < 0. {
            &[1]
        } else {
            &[0]
        };
        let mut best: Option<([i16; 3], [f32; 4], f32)> = None;
        for reach in [1, 4] {
            for dw in -reach / 2..=reach / 2 {
                for dy in -reach / 2..=reach / 2 {
                    for dx in -reach..=reach {
                        let samples = [center[0] + dw, center[1] + dx, center[2] + dy];
                        if !parities.contains(&(samples[1] & 1))
                            || samples.iter().any(|v| !(-32767..=32767).contains(v))
                        {
                            continue;
                        }
                        let samples = samples.map(|v| v as i16);
                        let Some(point) = decoded(1, &samples) else {
                            continue;
                        };
                        let error = self.error(point, frame);
                        if best.is_none_or(|(_, _, best)| error < best) {
                            best = Some((samples, point, error));
                        }
                    }
                }
            }
            if best.is_some_and(|(_, _, error)| error <= self.budget / 4.) {
                break;
            }
        }
        best
    }

    /// The tangent codes of a segment from key `a` to key `b`, if every frame between them stays
    /// within budget.
    fn segment(&self, keys: &[([i16; 3], [f32; 4], f32)], a: usize, b: usize) -> Option<Vec<u8>> {
        let width = width(self.kind);
        let span = (b - a) as f32;
        let at = |frame: usize| (frame - a) as f32 / span;
        // Rotation keys near z = 0 sit on a coarse grid. Between them a rotation follows its source
        // shifted by the keys' errors, blended across the segment, so its error drifts from one
        // key's to the next instead of springing back to the source next to each key.
        let target = |frame: usize, component: usize| {
            let goal = self.goal(frame, component);
            if self.kind != 1 {
                return goal;
            }
            let t = at(frame);
            let first = keys[a].1[component] - self.goal(a, component);
            let second = keys[b].1[component] - self.goal(b, component);
            goal + first * (1. - t) + second * t
        };
        let mut codes = vec![0x77u8; width];
        for (component, code) in codes.iter_mut().enumerate() {
            let (first, second) = (keys[a].1[component], keys[b].1[component]);
            // Least squares for the two chord offsets seeds a search of neighbouring codes.
            let (mut s11, mut s12, mut s22, mut b1, mut b2) = (0f64, 0f64, 0f64, 0f64, 0f64);
            for frame in a + 1..b {
                let t = f64::from(at(frame));
                let (h10, h11) = (t * t * t - 2. * t * t + t, t * t * t - t * t);
                let residual = f64::from(target(frame, component))
                    - f64::from(hermite(first, second, 0x77, at(frame)));
                s11 += h10 * h10;
                s12 += h10 * h11;
                s22 += h11 * h11;
                b1 += h10 * residual;
                b2 += h11 * residual;
            }
            let ridge = 1e-12;
            let determinant = (s11 + ridge) * (s22 + ridge) - s12 * s12;
            let start = nearest((b1 * (s22 + ridge) - b2 * s12) / determinant);
            let end = nearest((b2 * (s11 + ridge) - b1 * s12) / determinant);
            let mut best = (f32::INFINITY, 0x77);
            for high in start.saturating_sub(1)..=(start + 1).min(15) {
                for low in end.saturating_sub(1)..=(end + 1).min(15) {
                    let candidate = ((high << 4) | low) as u8;
                    let error = (a + 1..b)
                        .map(|frame| {
                            (hermite(first, second, candidate, at(frame))
                                - target(frame, component))
                            .abs()
                        })
                        .fold(0f32, f32::max);
                    if error < best.0 {
                        best = (error, candidate);
                    }
                }
            }
            *code = best.1;
        }
        for frame in a + 1..b {
            let mut point = [0.; 4];
            for (component, value) in point.iter_mut().enumerate().take(width) {
                *value = hermite(
                    keys[a].1[component],
                    keys[b].1[component],
                    codes[component],
                    at(frame),
                );
            }
            let within = if self.kind == 1 {
                let goal = [0, 1, 2, 3].map(|component| target(frame, component));
                let (length, norm) = (unit(point), unit(goal));
                length.is_finite()
                    && length > 1e-8
                    && point
                        .iter()
                        .zip(goal)
                        .all(|(value, goal)| (value / length - goal / norm).abs() <= self.budget)
            } else {
                self.error(point, frame) <= self.budget
            };
            if !within {
                return None;
            }
        }
        Some(codes)
    }
}

/// A track's stored form.
enum Samples {
    Raw(Vec<i16>),
    Curve {
        times: Vec<u8>,
        tangents: Vec<u8>,
        keys: Vec<i16>,
    },
}

struct Track {
    samples: Samples,
    interval: Option<(f32, f32)>,
    error: f32,
}

fn track(kind: usize, values: &[f32], frames: usize) -> Option<Track> {
    let interval = if kind == 1 {
        None
    } else {
        Some(interval(values)?)
    };
    let source = Source {
        kind,
        values,
        interval: interval.unwrap_or((1., 0.)),
        budget: budget(kind, interval.map_or(0., |(range, _)| range)),
        key_budget: key_budget(kind, interval.map_or(0., |(range, _)| range)),
    };
    let keys = (0..frames)
        .map(|frame| source.key(frame))
        .collect::<Option<Vec<_>>>()?;
    if keys.iter().any(|(_, _, error)| *error > source.key_budget) {
        return None;
    }
    let stored = stored(kind);
    let (ends, tangents) = segments(&source, &keys, frames);
    let segments = ends.len() - 1;
    let dense = 2 * (3 + ends.len() * stored) + segments + tangents.len() >= 2 * frames * stored;
    // What every frame decodes to before range, bias or normalization.
    let mut points = keys.iter().map(|key| key.1).collect::<Vec<_>>();
    if !dense {
        for (segment, pair) in ends.windows(2).enumerate() {
            for (step, point) in points[pair[0] + 1..pair[1]].iter_mut().enumerate() {
                let t = (step + 1) as f32 / (pair[1] - pair[0]) as f32;
                for (component, value) in point.iter_mut().enumerate().take(width(kind)) {
                    *value = hermite(
                        keys[pair[0]].1[component],
                        keys[pair[1]].1[component],
                        tangents[segment * width(kind) + component],
                        t,
                    );
                }
            }
        }
    }
    let deviations = points
        .iter()
        .enumerate()
        .map(|(frame, point)| source.deviation(*point, frame))
        .collect::<Option<Vec<_>>>()?;
    let error = deviations
        .iter()
        .flatten()
        .fold(0f32, |error, v| error.max(v.abs()));
    if kind == 1 {
        let motion = (0..4)
            .map(|component| {
                let (low, high) = (0..frames)
                    .map(|frame| source.goal(frame, component))
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), v| {
                        (low.min(v), high.max(v))
                    });
                high - low
            })
            .fold(0f32, f32::max);
        if error > (RELATIVE * motion).max(FLOOR) {
            return None;
        }
    }
    if kind == 1
        && deviations.windows(2).any(|pair| {
            pair[0]
                .iter()
                .zip(pair[1])
                .any(|(a, b)| (a - b).abs() > JITTER)
        })
    {
        return None;
    }
    let samples = if !dense {
        Samples::Curve {
            times: ends
                .windows(2)
                .map(|pair| u8::try_from(pair[1] - pair[0]).ok())
                .collect::<Option<_>>()?,
            tangents,
            keys: ends
                .iter()
                .flat_map(|&frame| keys[frame].0[..stored].to_vec())
                .collect(),
        }
    } else {
        Samples::Raw(
            keys.iter()
                .flat_map(|(samples, _, _)| samples[..stored].to_vec())
                .collect(),
        )
    };
    Some(Track {
        samples,
        interval,
        error,
    })
}

/// A codec 1 animated span's tables.
pub(super) struct Curve {
    counts: [u16; 3],
    raw: Vec<i16>,
    compressed: Vec<i16>,
    times: Vec<u8>,
    tangents: Vec<u8>,
    ranges: Vec<f32>,
    biases: Vec<f32>,
    mappings: Vec<i16>,
    error: f32,
    floats: usize,
}

impl Curve {
    /// The curve tables for `channels`, or `None` when a track cannot meet its budget or the
    /// tables exceed their signed-short offsets.
    pub(super) fn encode(frames: usize, channels: &[Channel; 3]) -> Option<Self> {
        if !(2..=3600).contains(&frames) {
            return None;
        }
        let mut curve = Self {
            counts: [0; 3],
            raw: Vec::new(),
            compressed: Vec::new(),
            times: Vec::new(),
            tangents: Vec::new(),
            ranges: Vec::new(),
            biases: Vec::new(),
            mappings: Vec::new(),
            error: 0.,
            floats: 0,
        };
        let mut intervals = [Vec::new(), Vec::new()];
        for (kind, channel) in channels.iter().enumerate() {
            let width = width(kind);
            if channel.slots.len() > 512
                || channel.values.len() != channel.slots.len() * frames * width
            {
                return None;
            }
            curve.counts[kind] = u16::try_from(channel.slots.len()).ok()?;
            curve.floats += channel.values.len() * 4;
            for values in channel.values.chunks_exact(frames * width) {
                let track = track(kind, values, frames)?;
                curve.error = curve.error.max(track.error);
                if let Some(interval) = track.interval {
                    intervals[usize::from(kind == 2)].push(interval);
                }
                match track.samples {
                    Samples::Raw(samples) => {
                        curve
                            .mappings
                            .push(i16::try_from(-(curve.raw.len() as i64) - 1).ok()?);
                        curve.raw.extend(samples);
                    }
                    Samples::Curve {
                        times,
                        tangents,
                        keys,
                    } => {
                        curve
                            .mappings
                            .push(i16::try_from(curve.compressed.len() + 1).ok()?);
                        curve.compressed.extend([
                            i16::try_from(curve.times.len()).ok()?,
                            i16::try_from(curve.tangents.len()).ok()?,
                            i16::try_from(times.len()).ok()?,
                        ]);
                        curve.compressed.extend(keys);
                        curve.times.extend(times);
                        curve.tangents.extend(tangents);
                    }
                }
            }
        }
        // Scale intervals come first, then position intervals, as the decoder indexes them.
        for (range, bias) in intervals.into_iter().flatten() {
            curve.ranges.push(range);
            curve.biases.push(bias);
        }
        // The last mapping is the compressed table's length, read unsigned.
        curve
            .mappings
            .push(u16::try_from(curve.compressed.len()).ok()? as i16);
        Some(curve)
    }

    /// Append the span after the clip's channel maps and point the animated span at it.
    pub(super) fn write(&self, out: &mut Vec<u8>) -> Result<()> {
        let shorts = |values: &[i16]| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let floats = |values: &[f32]| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let base = (out.len() + 11) & !7;
        out.resize(base - 4, 0);
        out.extend(0x80808F72u32.to_le_bytes());
        out.resize(base + 0x80, 0);
        put(out, 0x18, &(base as i64 - 0x18).to_le_bytes())?;
        put(out, base, &1u16.to_le_bytes())?;
        for (kind, count) in self.counts.iter().enumerate() {
            put(out, base + 2 + kind * 2, &count.to_le_bytes())?;
        }
        append(out, base + 0x10, 0x80800006, &shorts(&self.raw), 2)?;
        append(out, base + 0x20, 0x80800006, &shorts(&self.compressed), 2)?;
        append(out, base + 0x30, 0x80800009, &self.times, 1)?;
        append(out, base + 0x40, 0x80800009, &self.tangents, 1)?;
        append(out, base + 0x50, 0x8080000F, &floats(&self.ranges), 4)?;
        append(out, base + 0x60, 0x8080000F, &floats(&self.biases), 4)?;
        append(out, base + 0x70, 0x80800006, &shorts(&self.mappings), 2)?;
        let size = (out.len() - base + 11) & !7;
        out.resize(base - 4 + size, 0);
        // Stock spans carry an error and a compression ratio here, which the decoder does not read.
        put(out, base + 8, &self.error.to_le_bytes())?;
        put(
            out,
            base + 0xC,
            &(size as f32 / self.floats.max(1) as f32).to_le_bytes(),
        )?;
        put(out, 0x12C, &u32::try_from(size)?.to_le_bytes())?;
        Ok(())
    }
}

/// Whether the animated span written to `out` decodes to `channels` within the key budgets, which
/// with the interior rotation budget bound every frame.
pub(super) fn decodes(out: &[u8], frames: usize, channels: &[Channel; 3]) -> bool {
    use sundial::package_authoring::animation_curve::{Channel as Kind, Decoder};
    let Some(base) = out
        .get(0x18..0x20)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i64::from_le_bytes)
        .and_then(|offset| usize::try_from(0x18 + offset).ok())
    else {
        return false;
    };
    let counts = channels.each_ref().map(|channel| channel.slots.len());
    let Ok(decoder) = Decoder::read(out, base, counts, frames) else {
        return false;
    };
    for (kind, (channel, which)) in channels
        .iter()
        .zip([Kind::Scale, Kind::Rotation, Kind::Position])
        .enumerate()
    {
        let width = width(kind);
        for (track, values) in channel.values.chunks_exact(frames * width).enumerate() {
            let range = interval(values).map_or(0., |(range, _)| range);
            let budget = key_budget(kind, range) + if kind == 1 { ROTATION } else { 0. } + 1e-6;
            let Ok(samples) = decoder.samples(which, track) else {
                return false;
            };
            for (frame, point) in samples.iter().enumerate() {
                let source = &values[frame * width..(frame + 1) * width];
                let norm = if kind == 1 {
                    source.iter().map(|v| v * v).sum::<f32>().sqrt()
                } else {
                    1.
                };
                if point.iter().zip(source).any(|(value, expected)| {
                    let error = (value - expected / norm).abs();
                    error.is_nan() || error > budget
                }) {
                    return false;
                }
            }
        }
    }
    true
}

type Key = ([i16; 3], [f32; 4], f32);

fn segments(source: &Source<'_>, keys: &[Key], frames: usize) -> (Vec<usize>, Vec<u8>) {
    let kind = source.kind;
    let mut ends = vec![0];
    let mut tangents = Vec::new();
    while let Some(&start) = ends.last().filter(|&&start| start + 1 < frames) {
        let mut longest = (start + 1, vec![0x77; width(kind)]);
        let mut precise = None;
        let mut misses = 0;
        for end in start + 2..=(start + LONGEST).min(frames - 1) {
            if let Some(codes) = source.segment(keys, start, end) {
                if keys[end].2 <= source.budget || end == frames - 1 {
                    precise = Some((end, codes.clone()));
                }
                longest = (end, codes);
                misses = 0;
            } else {
                misses += 1;
                if misses > PATIENCE {
                    break;
                }
            }
        }
        // A key within budget ends the segment when it lies at least half as far as the longest
        // fit. Otherwise the longest fit does, because keys near z = 0 sit on a coarse grid and a
        // run of short segments between them steps from frame to frame.
        let best = match precise {
            Some(precise) if 2 * (precise.0 - start) >= longest.0 - start => precise,
            _ => longest,
        };
        ends.push(best.0);
        tangents.extend(best.1);
    }
    (ends, tangents)
}
