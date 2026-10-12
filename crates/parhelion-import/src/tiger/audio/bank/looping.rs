//! Authored native loops with paired lifecycle events and explicit parameter curves.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Start,
    Sustain,
    Stop,
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

fn node(w: &mut Write, parent: u32, bus: u32, layer: Option<&Layer>) -> Result<()> {
    w.bytes.extend([0; 3]);
    w.u32(bus);
    w.reference(parent, Kind::Object);
    w.u8(0);
    if let Some(layer) = layer {
        ensure!(
            layer.gain_db.is_finite() && (-96.0..=24.0).contains(&layer.gain_db),
            "Invalid loop layer gain"
        );
        w.bytes.extend([1, 0]);
        w.u32(layer.gain_db.to_bits());
    } else {
        w.u8(0);
    }
    w.u8(0);
    if parent == 0 {
        w.bytes.extend([0xd9, 1]);
        w.reference(3, Kind::Object);
    } else {
        w.u8(0xc0);
    }
    w.u8(0);
    w.bytes.extend([0, 1, 0, 0, 0, 0]);
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

impl Bank {
    /// Events 1 and 2 start and stop one object's complete layered sound.
    /// Starting again first retires this bank's preceding loops and tails.
    pub(crate) fn looping(layers: &[Layer], output_bus: u32, distance: [f32; 2]) -> Result<Self> {
        ensure!(
            !layers.is_empty() && layers.len() <= 128,
            "Invalid loop layer count"
        );
        ensure!(
            !matches!(output_bus, 0 | u32::MAX),
            "Loop output bus is unset"
        );
        ensure!(
            distance[0].is_finite()
                && distance[1].is_finite()
                && distance[0] >= 0.0
                && distance[1] > distance[0],
            "Invalid loop attenuation"
        );
        let mut hirc = Write::default();
        hirc.u32(0);
        let mut attenuation = Write::default();
        attenuation.reference(3, Kind::Object);
        attenuation.u8(0);
        attenuation.bytes.extend([0, 0, 0, 255, 255, 255, 255]);
        attenuation.u8(1);
        attenuation.u8(0);
        let mut curve = vec![[0.0, 0.0]];
        if distance[0] > 0.0 {
            curve.push([distance[0], 0.0]);
        }
        curve.push([distance[1], -96.0]);
        points(&mut attenuation, &curve)?;
        attenuation.u16(0);
        object(&mut hirc, 14, attenuation)?;
        let mut count = 1u32;
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
                node(&mut sound, container, 0, None)?;
                object(&mut hirc, 2, sound)?;
                count += 1;
            }
            let mut random = Write::default();
            random.reference(container, Kind::Object);
            node(&mut random, 0, output_bus, Some(layer))?;
            random.u16(if matches!(layer.phase, Phase::Sustain) {
                0
            } else {
                1
            });
            random.bytes.extend([0; 16]);
            random.u16(1);
            random.bytes.extend([4, 0, 0, 0x1a]);
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
            objects: (1..next).collect(),
            refs: out.refs,
        })
    }
}
