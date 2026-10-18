//! A native PCM event that selects one complete recording per trigger.
use super::*;

pub(crate) struct Layer {
    pub media: Vec<u32>,
    pub gain_db: f32,
    pub voice_limit: u16,
}

fn node(w: &mut Write, parent: u32, bus: u32, gain: Option<f32>, voice_limit: u16, global: bool) {
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
    // Override the actor parent at the container. Bit 2 selects a global limit.
    // Kill the oldest voice at the limit. Child variations inherit the shared limit.
    w.bytes.extend([
        if voice_limit > 0 {
            8 | if global { 4 } else { 0 }
        } else {
            0
        },
        1,
    ]);
    w.u16(voice_limit);
    w.bytes.extend([0, 0]);
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
    pub fn one_shot(
        media: &[u32],
        output_bus: u32,
        gain_db: f32,
        voice_limit: u16,
    ) -> Result<Self> {
        Self::shot(media, output_bus, gain_db, voice_limit, false)
    }

    /// Share the private container's voice limit across transient game objects.
    pub fn one_shot_global(
        media: &[u32],
        output_bus: u32,
        gain_db: f32,
        voice_limit: u16,
    ) -> Result<Self> {
        Self::shot(media, output_bus, gain_db, voice_limit, true)
    }

    fn shot(
        media: &[u32],
        output_bus: u32,
        gain_db: f32,
        voice_limit: u16,
        global: bool,
    ) -> Result<Self> {
        Self::shots(
            &[Layer {
                media: media.to_vec(),
                gain_db,
                voice_limit,
            }],
            output_bus,
            global,
        )
    }

    /// Trigger one independent variation from every simultaneous source layer.
    pub(crate) fn one_shot_layers(layers: &[Layer], output_bus: u32) -> Result<Self> {
        Self::shots(layers, output_bus, false)
    }

    fn shots(layers: &[Layer], output_bus: u32, global: bool) -> Result<Self> {
        ensure!(
            !layers.is_empty() && layers.len() <= 128,
            "Invalid one-shot layer count"
        );
        ensure!(
            output_bus != 0 && output_bus != u32::MAX,
            "Invalid native output bus"
        );
        let count = layers
            .iter()
            .map(|layer| layer.media.len() + 2)
            .sum::<usize>()
            + 1;
        ensure!(count <= 65536, "Layered sound exceeds native object limit");
        let mut hirc = Write::default();
        hirc.u32(count as u32);
        let mut next = 2;
        let mut actions = Vec::new();
        for layer in layers {
            actions.push(next);
            next = append_layer(&mut hirc, layer, output_bus, next, global)?;
        }
        let mut event = Write::default();
        event.reference(1, Kind::Object);
        event.u32(actions.len() as u32);
        for action in actions {
            event.reference(action, Kind::Object);
        }
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
            objects: (1..next).collect(),
            refs: out.refs,
        })
    }
}

fn append_layer(
    hirc: &mut Write,
    layer: &Layer,
    output_bus: u32,
    first: u32,
    global: bool,
) -> Result<u32> {
    let Layer {
        media,
        gain_db,
        voice_limit,
    } = layer;
    ensure!(
        (1..=1000).contains(voice_limit),
        "Invalid one-shot voice limit"
    );
    ensure!(
        !media.is_empty() && media.len() <= 4096,
        "Invalid one-shot variation count"
    );
    ensure!(
        gain_db.is_finite() && (-96.0..=24.0).contains(gain_db),
        "Invalid one-shot gain"
    );
    let count = u32::try_from(media.len())?;
    let container = first + 1;
    let media_start = first + 2;
    for (i, medium) in media.iter().enumerate() {
        let mut sound = Write::default();
        sound.reference(i as u32 + media_start, Kind::Object);
        sound.u32(0x0001_0001); // PCM.
        sound.u8(2); // Fully streamed.
        sound.reference(*medium, Kind::Media);
        sound.reference(*medium, Kind::MediaSize);
        sound.u8(0);
        node(&mut sound, container, 0, None, 0, false);
        append(hirc, 2, sound)?;
    }
    let mut random = Write::default();
    random.reference(container, Kind::Object);
    node(
        &mut random,
        0,
        output_bus,
        Some(*gain_db),
        *voice_limit,
        global,
    );
    random.u16(1); // One loop.
    random.bytes.extend([0; 22]); // Step random, no transitions or continuous playback.
    random.u32(count);
    for id in media_start..count + media_start {
        random.reference(id, Kind::Object);
    }
    random.u16(count as u16);
    for id in media_start..count + media_start {
        random.reference(id, Kind::Object);
        random.u32(50_000);
    }
    append(hirc, 5, random)?;
    let mut action = Write::default();
    action.reference(first, Kind::Object);
    action.u16(0x0403); // Play on the triggering game object.
    action.reference(container, Kind::Object);
    action.bytes.extend([0, 0, 0, 4]);
    action.reference(1, Kind::Bank);
    append(hirc, 3, action)?;
    Ok(count + media_start)
}
