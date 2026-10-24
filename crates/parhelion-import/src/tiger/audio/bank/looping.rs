//! Authored native loops with paired lifecycle events and explicit parameter curves.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Start,
    Sustain,
    Stop,
    /// Plays a variation at random for as long as the sound sustains, waiting a random time from
    /// `gap` (seconds) after each one ends.
    Detail {
        gap: [f32; 2],
    },
}

pub(crate) struct Curve {
    pub parameter: u32,
    /// Native parameter 0 is volume in dB and 2 is pitch in cents.
    pub target: u8,
    pub points: Vec<[f32; 2]>,
}

pub(crate) struct Layer {
    pub media: Vec<u32>,
    pub gain_db: f32,
    pub phase: Phase,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
    pub curves: Vec<Curve>,
}

fn object(hirc: &mut Write, kind: u8, payload: Write) -> Result<()> {
    hirc.u8(kind);
    hirc.u32(u32::try_from(payload.bytes.len())?);
    hirc.append(payload);
    Ok(())
}

fn points(w: &mut Write, points: &[[f32; 2]]) -> Result<()> {
    ensure!((2..=256).contains(&points.len()), "Invalid loop curve size");
    ensure!(
        points.iter().flatten().all(|v| v.is_finite())
            && points.windows(2).all(|p| p[0][0] < p[1][0]),
        "Loop curve points must be finite and ordered"
    );
    w.u16(u16::try_from(points.len())?);
    for point in points {
        w.u32(point[0].to_bits());
        w.u32(point[1].to_bits());
        w.u32(4); // Linear interpolation, including the unused final mode.
    }
    Ok(())
}

/// A top container's priority and the offset its priority takes at its attenuation's farthest
/// distance, as Distant Shore's ambient actor-mixer (`1A50F6C0`) sets them, so the nearest
/// instances keep playing within an instance limit.
const LIMITED_PRIORITY: f32 = 50.0;
const LIMITED_PRIORITY_OFFSET: f32 = -39.0;

/// Writes a node's base parameters. A top container (`parent` 0) with a `limit` plays at most that
/// many instances at once, the rest virtual, by priority falling with distance.
fn node(
    w: &mut Write,
    parent: u32,
    bus: u32,
    layer: Option<&Layer>,
    spatial: bool,
    limit: Option<u16>,
) -> Result<()> {
    let limit = limit.filter(|_| parent == 0);
    w.bytes.extend([0; 3]);
    w.u32(bus);
    w.reference(parent, Kind::Object);
    // Priority overrides the parent's and applies the distance factor.
    w.u8(if limit.is_some() { 3 } else { 0 });
    if let Some(layer) = layer {
        ensure!(
            layer.gain_db.is_finite() && (-96.0..=24.0).contains(&layer.gain_db),
            "Invalid loop layer gain"
        );
        // Properties 0 (volume in dB), 6 (priority) and 7 (priority distance offset).
        if limit.is_some() {
            w.bytes.extend([3, 0, 6, 7]);
            for value in [layer.gain_db, LIMITED_PRIORITY, LIMITED_PRIORITY_OFFSET] {
                w.u32(value.to_bits());
            }
        } else {
            w.bytes.extend([1, 0]);
            w.u32(layer.gain_db.to_bits());
        }
    } else {
        w.u8(0);
    }
    w.u8(0);
    if parent == 0 && spatial {
        w.bytes.extend([0xd9, 1]);
        w.reference(3, Kind::Object);
    } else {
        w.u8(if parent == 0 { 0xc1 } else { 0xc0 });
    }
    w.u8(0);
    // Advanced settings: flags, virtual voice queue behavior, instance limit and below-threshold
    // behavior. A limited container keeps its own limit and sends the rest virtual (`1E`, 1, n,
    // 2), as Distant Shore's ambient actor-mixer does with 6.
    match limit {
        Some(instances) => {
            ensure!(instances > 0, "Invalid loop instance limit");
            w.bytes.extend([0x1E, 1]);
            w.u16(instances);
            w.bytes.extend([2, 0]);
        }
        None => w.bytes.extend([0, 1, 0, 0, 0, 0]),
    }
    w.u32(0);
    let curves = layer
        .map(|layer| layer.curves.as_slice())
        .unwrap_or_default();
    w.u16(u16::try_from(curves.len())?);
    for (index, curve) in curves.iter().enumerate() {
        ensure!(
            curve.parameter != 0 && matches!(curve.target, 0 | 2),
            "Unsupported loop parameter"
        );
        w.u32(curve.parameter); // Per-object native game parameters are not HIRC IDs.
        w.bytes.extend([0, 1, curve.target]);
        w.u32(index as u32 + 1);
        w.u8(0); // Values are explicit dB or cents, with no hidden curve scaling.
        points(w, &curve.points)?;
    }
    Ok(())
}

fn action(hirc: &mut Write, id: u32, target: u32, play: bool, fade_ms: u32) -> Result<()> {
    ensure!(fade_ms <= 60_000, "Loop fade exceeds one minute");
    let mut w = Write::default();
    w.reference(id, Kind::Object);
    w.u16(if play { 0x0403 } else { 0x0103 });
    w.reference(target, Kind::Object);
    w.u8(0);
    w.bytes.extend([1, 0x0f]);
    w.u32(fade_ms);
    w.bytes.extend([0, 4]);
    if play {
        w.reference(1, Kind::Bank);
    } else {
        w.u32(0); // No stop exceptions.
    }
    object(hirc, 3, w)
}

/// Writes attenuation 3, whose dry volume follows `curve`, distance in meters to dB.
fn attenuation(hirc: &mut Write, curve: Option<&[[f32; 2]]>) -> Result<u32> {
    let Some(curve) = curve else {
        return Ok(0);
    };
    ensure!(
        curve.first().is_some_and(|point| point[0] == 0.0)
            && curve.iter().all(|point| (-96.0..=0.0).contains(&point[1])),
        "Invalid loop attenuation"
    );
    let mut payload = Write::default();
    payload.reference(3, Kind::Object);
    payload.u8(0);
    payload.bytes.extend([0, 0, 0, 255, 255, 255, 255]);
    payload.u8(1);
    payload.u8(0);
    points(&mut payload, curve)?;
    payload.u16(0);
    object(hirc, 14, payload)?;
    Ok(1)
}

impl Bank {
    /// Events 1 and 2 start and stop one object's complete layered sound.
    /// Starting again first retires this bank's preceding loops and tails.
    pub(crate) fn looping(layers: &[Layer], output_bus: u32, distance: [f32; 2]) -> Result<Self> {
        ensure!(
            distance[0].is_finite()
                && distance[1].is_finite()
                && distance[0] >= 0.0
                && distance[1] > distance[0],
            "Invalid loop attenuation"
        );
        let mut curve = vec![[0.0, 0.0]];
        if distance[0] > 0.0 {
            curve.push([distance[0], 0.0]);
        }
        curve.push([distance[1], -96.0]);
        Self::looped(layers, output_bus, Some(&curve), None)
    }

    /// As [`Bank::looping`], with its volume following `curve` over distance, meters to dB from 0
    /// to -96, the last point its farthest reach, and each layer playing at most `limit`
    /// instances at once, the nearest.
    pub(crate) fn looping_curve(
        layers: &[Layer],
        output_bus: u32,
        curve: &[[f32; 2]],
        limit: Option<u16>,
    ) -> Result<Self> {
        Self::looped(layers, output_bus, Some(curve), limit)
    }

    pub(crate) fn looping_nonspatial(layers: &[Layer], output_bus: u32) -> Result<Self> {
        Self::looped(layers, output_bus, None, None)
    }

    #[allow(clippy::cognitive_complexity)]
    fn looped(
        layers: &[Layer],
        output_bus: u32,
        curve: Option<&[[f32; 2]]>,
        limit: Option<u16>,
    ) -> Result<Self> {
        ensure!(
            !layers.is_empty() && layers.len() <= 128,
            "Invalid loop layer count"
        );
        ensure!(
            !matches!(output_bus, 0 | u32::MAX),
            "Loop output bus is unset"
        );
        let mut hirc = Write::default();
        hirc.u32(0);
        let mut count = attenuation(&mut hirc, curve)?;
        let mut next = 4u32;
        let mut starts = Vec::new();
        let mut stops = Vec::new();
        for layer in layers {
            ensure!(
                !layer.media.is_empty() && layer.media.len() <= 4096,
                "Invalid loop variation count"
            );
            let container = next;
            next += 1;
            let mut sounds = Vec::new();
            for medium in &layer.media {
                let id = next;
                next += 1;
                sounds.push(id);
                let mut sound = Write::default();
                sound.reference(id, Kind::Object);
                sound.u32(0x00010001);
                sound.u8(2);
                sound.reference(*medium, Kind::Media);
                sound.reference(*medium, Kind::MediaSize);
                sound.u8(0);
                node(&mut sound, container, 0, None, curve.is_some(), None)?;
                object(&mut hirc, 2, sound)?;
                count += 1;
            }
            let mut random = Write::default();
            random.reference(container, Kind::Object);
            node(
                &mut random,
                0,
                output_bus,
                Some(layer),
                curve.is_some(),
                limit,
            )?;
            random.u16(
                if matches!(layer.phase, Phase::Sustain | Phase::Detail { .. }) {
                    0
                } else {
                    1
                },
            );
            // Loop count variation, then the transition: its time and random range in
            // milliseconds, the avoided repeats and its mode, 4 joining variations sample
            // accurately and 3 waiting the transition time after each.
            random.bytes.extend([0; 4]);
            let mode = if let Phase::Detail { gap } = layer.phase {
                ensure!(
                    gap.iter().all(|seconds| (0.0..=600.0).contains(seconds)) && gap[0] <= gap[1],
                    "Invalid detail gap"
                );
                for value in [gap[0] * 1000., 0., (gap[1] - gap[0]) * 1000.] {
                    random.u32(f32::to_bits(value));
                }
                3
            } else {
                random.bytes.extend([0; 12]);
                4
            };
            random.u16(1);
            random.bytes.extend([mode, 0, 0, 0x1a]);
            random.u32(sounds.len() as u32);
            for id in &sounds {
                random.reference(*id, Kind::Object);
            }
            random.u16(sounds.len() as u16);
            for id in &sounds {
                random.reference(*id, Kind::Object);
                random.u32(50_000);
            }
            object(&mut hirc, 5, random)?;
            // Stop only these private objects, never every voice on the game object.
            let cancel = next;
            action(&mut hirc, cancel, container, false, 0)?;
            starts.push(cancel);
            next += 1;
            let play = next;
            action(&mut hirc, play, container, true, layer.fade_in_ms)?;
            next += 1;
            if matches!(layer.phase, Phase::Stop) {
                stops.push(play);
            } else {
                starts.push(play);
            }
            if !matches!(layer.phase, Phase::Stop) {
                action(&mut hirc, next, container, false, layer.fade_out_ms)?;
                stops.push(next);
                next += 1;
                count += 1;
            }
            count += 3;
        }
        for (id, actions) in [(1, starts), (2, stops)] {
            let mut event = Write::default();
            event.reference(id, Kind::Object);
            event.u32(actions.len() as u32);
            for action in actions {
                event.reference(action, Kind::Object);
            }
            object(&mut hirc, 4, event)?;
            count += 1;
        }
        hirc.bytes[..4].copy_from_slice(&count.to_le_bytes());
        let mut header = Write::default();
        header.u32(113);
        header.reference(1, Kind::Bank);
        header.bytes.extend([0; 16]);
        let mut out = Write::default();
        out.chunk(b"BKHD", header)?;
        out.chunk(b"HIRC", hirc)?;
        Ok(Self {
            bytes: out.bytes,
            bank_id: 1,
            // Object 3 is reserved for attenuation and is absent from nonspatial banks.
            objects: (1..next).filter(|id| *id != 3 || curve.is_some()).collect(),
            refs: out.refs,
        })
    }
}
