//! A native PCM event that selects one complete recording per trigger.
use super::*;

fn node(w: &mut Write, parent: u32, bus: u32, gain: Option<f32>) {
    // Native v113 common node fields, without effects, auxiliary sends or RTPCs.
    w.bytes.extend([0; 3]);
    w.u32(bus); // The selected native output bus stays in the global namespace.
    w.reference(parent, Kind::Object);
    w.u8(0);
    if let Some(gain) = gain {
        w.bytes.extend([1, 0]); // Volume property.
        w.u32(gain.to_bits());
    } else {
        w.u8(0);
    }
    w.u8(0); // No randomized properties.
    w.u8(if parent == 0 { 0xc1 } else { 0xc0 }); // Nonspatial root, inherited children.
    w.u8(0);
    w.bytes.extend([0, 1, 0, 0, 0, 0]); // Native voice limits and virtual queue settings.
    w.u32(0); // States.
    w.u16(0); // RTPCs.
}

fn append(hirc: &mut Write, kind: u8, payload: Write) -> Result<()> {
    hirc.u8(kind);
    hirc.u32(u32::try_from(payload.bytes.len())?);
    hirc.append(payload);
    Ok(())
}

impl Bank {
    /// Build v113 event 1 with one random choice per trigger on an explicit native bus.
    /// This does not supply spatial attenuation, environment switching or source pitch curves.
    pub fn one_shot(media: &[u32], output_bus: u32, gain_db: f32) -> Result<Self> {
        ensure!(
            !media.is_empty() && media.len() <= 4096,
            "Invalid one-shot variation count"
        );
        ensure!(
            output_bus != 0 && output_bus != u32::MAX,
            "Invalid native output bus"
        );
        ensure!(
            gain_db.is_finite() && (-96.0..=24.0).contains(&gain_db),
            "Invalid one-shot gain"
        );
        let count = u32::try_from(media.len())?;
        let mut hirc = Write::default();
        hirc.u32(count + 3);
        for (i, medium) in media.iter().enumerate() {
            let mut sound = Write::default();
            sound.reference(i as u32 + 4, Kind::Object);
            sound.u32(0x0001_0001); // PCM.
            sound.u8(2); // Fully streamed.
            sound.reference(*medium, Kind::Media);
            sound.reference(*medium, Kind::MediaSize);
            sound.u8(0);
            node(&mut sound, 3, 0, None);
            append(&mut hirc, 2, sound)?;
        }
        let mut random = Write::default();
        random.reference(3, Kind::Object);
        node(&mut random, 0, output_bus, Some(gain_db));
        random.u16(1); // One loop.
        random.bytes.extend([0; 22]); // Step random, no transitions or continuous playback.
        random.u32(count);
        for id in 4..count + 4 {
            random.reference(id, Kind::Object);
        }
        random.u16(count as u16);
        for id in 4..count + 4 {
            random.reference(id, Kind::Object);
            random.u32(50_000);
        }
        append(&mut hirc, 5, random)?;
        let mut action = Write::default();
        action.reference(2, Kind::Object);
        action.u16(0x0403); // Play on the triggering game object.
        action.reference(3, Kind::Object);
        action.bytes.extend([0, 0, 0, 4]);
        action.reference(1, Kind::Bank);
        append(&mut hirc, 3, action)?;
        let mut event = Write::default();
        event.reference(1, Kind::Object);
        event.u32(1);
        event.reference(2, Kind::Object);
        append(&mut hirc, 4, event)?;
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
            objects: (1..count + 4).collect(),
            refs: out.refs,
        })
    }
}
