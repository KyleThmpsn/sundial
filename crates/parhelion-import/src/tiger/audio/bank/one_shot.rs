//! A native PCM event that selects one complete recording per trigger.
use super::*;

pub(crate) struct Layer {
    pub media: Vec<u32>,
    pub gain_db: f32,
    pub voice_limit: u16,
}

/// A native attenuation (HIRC type 14) copied from a version 113 bank, with the positioning bytes
/// of the node that names it. A generated root names a private copy, so its playback falls off
/// with distance as that node's does.
pub(crate) struct Attenuation {
    /// The positioning flags and their 3D byte, `D9 01` in the inspected native contact hierarchy.
    pub positioning: [u8; 2],
    /// The object body after its identity.
    pub body: Vec<u8>,
}

impl Attenuation {
    /// Checks that `body` is a complete version 113 attenuation without a cone or parameter curves,
    /// so the copy names nothing outside its bank: the cone byte, seven curve selectors, the curve
    /// count and each curve's scaling, point count and 12-byte points, then no RTPCs.
    pub(crate) fn new(positioning: [u8; 2], body: Vec<u8>) -> Result<Self> {
        ensure!(
            positioning[0] == 0xd9,
            "Native attenuation node positioning differs"
        );
        let mut r = Read {
            bytes: &body,
            at: 0,
        };
        ensure!(r.u8()? == 0, "Native attenuation has a cone");
        let selectors = r.take(7)?.to_vec();
        let curves = usize::from(r.u8()?);
        ensure!(
            selectors
                .iter()
                .all(|&curve| curve == u8::MAX || usize::from(curve) < curves),
            "Native attenuation selects an absent curve"
        );
        for _ in 0..curves {
            r.u8()?;
            let points = usize::from(r.u16()?);
            ensure!(points > 0, "Native attenuation curve has no points");
            for point in r.take(points * 12)?.chunks_exact(12) {
                ensure!(
                    point[..8]
                        .chunks_exact(4)
                        .all(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]).is_finite()),
                    "Native attenuation curve is not finite"
                );
            }
        }
        ensure!(r.u16()? == 0, "Native attenuation has parameter curves");
        ensure!(r.at == body.len(), "Native attenuation has trailing data");
        Ok(Self { positioning, body })
    }
}

fn node(
    w: &mut Write,
    parent: u32,
    bus: u32,
    gain: Option<f32>,
    voice_limit: u16,
    global: bool,
    attenuation: Option<([u8; 2], u32)>,
) {
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
    match attenuation.filter(|_| parent == 0) {
        // A spatial root names its attenuation as the native node does. Children inherit it.
        Some((positioning, id)) => {
            w.bytes.extend(positioning);
            w.reference(id, Kind::Object);
        }
        None => w.u8(if parent == 0 { 0xc1 } else { 0xc0 }), // Nonspatial root, inherited children.
    }
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

    /// As [`Self::one_shot_global`], with a private copy of a native attenuation on the root, so
    /// the sound is positioned and falls off with distance.
    pub(crate) fn one_shot_global_spatial(
        media: &[u32],
        output_bus: u32,
        gain_db: f32,
        voice_limit: u16,
        attenuation: &Attenuation,
    ) -> Result<Self> {
        Self::shots(
            &[Layer {
                media: media.to_vec(),
                gain_db,
                voice_limit,
            }],
            output_bus,
            true,
            Some(attenuation),
        )
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
            None,
        )
    }

    /// Move the bank's own identities to a separate local range so several generated banks can
    /// share one linked namespace, which requires distinct bank and object identities. Object
    /// `n` becomes `base + n` and the bank becomes `base`. External identities stay unchanged.
    pub fn rebased(mut self, base: u32) -> Result<Self> {
        let last = self.objects.iter().copied().max().unwrap_or(0);
        ensure!(
            base > 0 && base.checked_add(last).is_some(),
            "Invalid generated bank identity range"
        );
        for reference in &mut self.refs {
            let value = match reference.kind {
                Kind::Object if self.objects.contains(&reference.value) => base + reference.value,
                Kind::Bank if reference.value == self.bank_id => base,
                _ => continue,
            };
            reference.value = value;
            self.bytes[reference.offset..reference.offset + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        self.objects = self.objects.iter().map(|id| base + id).collect();
        self.bank_id = base;
        Ok(self)
    }

    /// Trigger one independent variation from every simultaneous source layer.
    pub(crate) fn one_shot_layers(layers: &[Layer], output_bus: u32) -> Result<Self> {
        Self::shots(layers, output_bus, false, None)
    }

    fn shots(
        layers: &[Layer],
        output_bus: u32,
        global: bool,
        attenuation: Option<&Attenuation>,
    ) -> Result<Self> {
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
            + 1
            + usize::from(attenuation.is_some());
        ensure!(count <= 65536, "Layered sound exceeds native object limit");
        let mut hirc = Write::default();
        hirc.u32(count as u32);
        let mut next = 2;
        // The attenuation precedes the nodes that name it, as in native banks.
        let positioning = match attenuation {
            Some(attenuation) => {
                let mut object = Write::default();
                object.reference(next, Kind::Object);
                object.bytes.extend(&attenuation.body);
                append(&mut hirc, 14, object)?;
                next += 1;
                Some((attenuation.positioning, next - 1))
            }
            None => None,
        };
        let mut actions = Vec::new();
        for layer in layers {
            actions.push(next);
            next = append_layer(&mut hirc, layer, output_bus, next, global, positioning)?;
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
    attenuation: Option<([u8; 2], u32)>,
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
        node(&mut sound, container, 0, None, 0, false, None);
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
        attenuation,
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
