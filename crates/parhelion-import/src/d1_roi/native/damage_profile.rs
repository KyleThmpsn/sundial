//! Rise of Iron area damage profiles become private native outgoing profiles.
//!
//! The admitted family is Rise of Iron's `damage_impulse` definition `80AAC28B` with runtime
//! `80AAC28C`, which Firefly, Incendiary Rounds, Negative Rounds and Ice Breaker name from their
//! victim queries (`80A7F372`, `80A7F351`), as do Wrath Conductors, Accelerant and Cursebringer.
//! Its fixed root is the same across the family, and the payload size follows the response,
//! audio-row and category arrays. Native graph `8152BB5C` names profile `8152BB69` from the same
//! query position. Its definition supplies the native envelope, and the source values go to the
//! following counterpart offsets.
//!
//! | Role | Source root (`90`) | Native root (`A0`) |
//! | --- | --- | --- |
//! | Lower, upper amount, random extent | `+80`, `+84`, `+88` | `+14`, `+18`, `+1C` |
//! | Damage type (same encoding) | `+8C`, repeated at `+C0` | `+20` |
//! | Category mask of the category rows | `+90..+AF`, 256 bits | `+24..+4B`, 320 bits |
//! | Response pointer list | `+28` | `+78` |
//! | Category rows | `+C8` | `+60` |
//! | Float, word (family `3.5`, 1) | `+B0`, `+B4` | `+4C`, `+50` |
//! | Exemption flags, bit 6 scale | `+D8`, `+DC` | `+70`, `+74` |
//! | Victim indicator byte | `+F0` | `+A0` |
//! | Ownerless kill attribution record | `+B8`, `80802DAA` | `+58`, `80804B17` |
//! | Float32 one | `+50` | `+D0` |
//! | Feedback | `+100` | `+B0` |
//!
//! Each build compiles its category mask from the category rows against its own registry, so the
//! native mask is rebuilt from the rows. Native word `+54 = 100` has no consumer-qualified role
//! and stays. The Explosive Rounds profile `80AA2DC2`
//! of Rise of Iron hand cannon spawn `80A6A859` holds `1.0` and `0` at
//! `+B0/+B4`, which carry to `+4C/+50`. Responses: push `808023B0` and `80802B30` keep
//! identical bodies as `80804B57` and `80804B56`. Audio list `808021B3` rows (`28`) become
//! `80804B48` rows (`38`): bytes `+00..+20` are kept, native's optional third resource `+28` is
//! absent and the control word moves from `+20` to `+30`. Control byte `42` becomes `52`, as in
//! both examined stock pairs. Its native consumer is unqualified. Both builds look a response up
//! by class, take the first and skip an absent one (`005E0220`, `00B8C890`), so a profile may
//! hold any of them in any order, or none, as stock profiles do.
use super::*;
use crate::{
    d1_roi::{
        audio as graph_audio,
        dependency::{Asset, Patch},
    },
    presentation::{append, put},
    tiger::sequence as native_sequence,
};
pub(crate) mod feedback;
mod health;

/// The stock outgoing profile every private translation starts from.
pub(crate) const TEMPLATE: u32 = 0x8152BB69;
/// The label dictionaries damage packets carry their categories against, in each build.
pub(crate) const SOURCE_DICTIONARY: u32 = 0x80AAC26E;
pub(crate) const NATIVE_DICTIONARY: u32 = 0x80C70CA1;
const NATIVE_CUE: u32 = 0x80809802;

pub struct Translated {
    pub assets: Vec<Asset>,
    /// Source cues the audio response names, published by the caller's cue closure.
    pub cues: BTreeSet<u32>,
    /// Source category mask words `+90` and `+A8`, reported beside the rebuilt native mask.
    pub unqualified: [u32; 2],
    /// Source categories Shadowkeep's label registry lacks, which the translation leaves out.
    pub absent: Vec<u32>,
    /// Source cues of audio rows that play nothing, which the translation leaves out.
    pub silent: Vec<u32>,
}

fn word(p: &Payload, at: usize, value: u32) -> Result<bool> {
    Ok(p.u32(at)? == value)
}

pub fn translate(
    source: &Source,
    native: &mut Reader,
    tag: u32,
    prefix: &str,
) -> Result<Translated> {
    ensure!(
        source.reference(tag)? == 0x80800861,
        "Source area profile is not a component owner"
    );
    let p = source.read(tag)?;
    // A profile event's healing owner has the direct healing family's envelope.
    if p.0.len() == 0x230 {
        return health::translate(source, native, tag, prefix);
    }
    // The family's runtime and root classes fix its fields. Its payload size follows its
    // response, audio-row and category arrays.
    let (runtime, root) = (p.pointer(16)?, p.pointer(24)?);
    ensure!(
        runtime == 0x68
            && root == 0x90
            && p.u32(runtime - 4)? == 0x80AA_C28C
            && p.u32(root - 4)? == 0x80AA_C28B,
        "Source area profile has another envelope"
    );
    let lower = p.f32(root + 0x80)?;
    let upper = p.f32(root + 0x84)?;
    let kind = p.u32(root + 0x8C)?;
    ensure!(
        lower.is_finite()
            && upper.is_finite()
            && 0.0 <= lower
            && lower <= upper
            && p.u32(root + 0x88)? == 0
            && kind <= 3
            && word(&p, root + 0xC0, kind)?,
        "Source area profile amount or damage type is unqualified"
    );
    // The remaining definition fields with a native counterpart are fixed across the admitted
    // family, apart from `+B0/+B4`, which carry over.
    let (scale, flag) = (p.f32(root + 0xB0)?, p.u32(root + 0xB4)?);
    ensure!(
        scale.is_finite() && flag <= 1,
        "Source area profile field B0 or B4 is unqualified"
    );
    // The description, exemptions and victim response sit where the root's relative fields name
    // them. The scalar modifiers, behaviors, attacker responses and routing bytes have no
    // translation and must be empty.
    ensure!(
        p.pointer(root + 0x18)? == root + 0x80
            && p.pointer(root + 0x20)? == root + 0xD8
            && p.pointer(root + 0x38)? == root + 0xF0
            && p.u64(root + 0x60)? == 0
            && p.u64(root + 0x70)? == 0
            && p.u64(root + 0xE0)? == 0
            && p.u64(root + 0x108)? == 0,
        "Source area profile has a field without a translation"
    );
    // The description's optional `80802DAA` record attributes a kill when the damage has no owner
    // entity: a cause name, a killer name and three flag bytes. Source getter `005DFA20` feeds
    // `006D3440`, which ranks reassignment by `+A` (`006D4030`), builds the attribution from `+4`,
    // `+8` and `+A` (`0067D250`) and gives the death screen `+4`, `+9` and, unless empty, `+0`
    // (`00976C00`). Native root `+58` names an `80804B17` record read the same way at the same
    // offsets (`00D7DE90`, `00F93650`, `013A0790`), so it carries byte for byte.
    let attribution = if p.u64(root + 0xB8)? == 0 {
        None
    } else {
        let at = p.pointer(root + 0xB8)?;
        ensure!(
            at >= 4 && p.u32(at - 4)? == 0x80802DAA && p.u8(at + 0x0B)? == 0,
            "Source area profile attribution record differs"
        );
        Some(p.bytes::<0x0C>(at)?)
    };
    // Exemption flags `+D8` carry bit for bit to `+70`. Source `005ACBA0`, through getter
    // `005DDBB0`, and native `00B87740`, from `00B83E10`, read the low byte and test the same
    // eight relationship bits as the query masks, zeroing the damage of an exempt recipient.
    // Bit 6 reads its scale at source `+DC` and native `+74`.
    let flags = p.u32(root + 0xD8)?;
    ensure!(
        flags <= 0xFF,
        "Source area profile exemption flags exceed their byte"
    );
    // The victim response's first byte keeps damage indicators from pointing at the damage owner
    // (source getter `005DF7E0`). Native moves the whole response `18` bytes later, its feedback
    // from `+100` to `+B0` and this byte from `+F0` to `+A0`. Stock pairs agree, which is inferred.
    let indicator = p.u8(root + 0xF0)?;
    ensure!(
        indicator <= 1 && p.bytes::<15>(root + 0xF1)? == [0; 15] && p.u32(root + 0x104)? == 0,
        "Source area profile victim response has other fields"
    );
    for (field, value) in [(0x50, 1.0f32.to_bits()), (0xDC, 1.0f32.to_bits())] {
        ensure!(
            word(&p, root + field, value)?,
            "Source area profile field {field:X} differs from the qualified family"
        );
    }
    let feedback = p.u32(root + 0x100)?;
    let Responses {
        push,
        fixed,
        mut audio,
    } = read_responses(&p, root)?;
    let AudioRows { cues, rows, silent } = audio_rows(source, &p, audio)?;
    // An audio response whose every row was silent plays nothing and leaves with its rows.
    if rows.is_empty() && !silent.is_empty() {
        audio = None;
    }
    let categories = p
        .array(root + 0xC8, 0x18, Some(0x80800365))?
        .into_iter()
        .map(|row| {
            ensure!(
                p.bytes::<12>(row + 4)? == [0; 12]
                    && word(&p, row + 0x10, SOURCE_DICTIONARY)?
                    && p.u32(row + 0x14)? == 0,
                "Source area profile category row differs"
            );
            p.u32(row)
        })
        .collect::<Result<Vec<_>>>()?;

    let template = native.tag(TEMPLATE, Some(0x80809C36))?;
    let (n_runtime, n_root) = (template.pointer(16)?, template.pointer(24)?);
    ensure!(
        n_runtime == 0x70
            && n_root == 0xA0
            && template.u32(n_root - 4)? == 0x80804B02
            && template.u32(n_root + 0x24)? == 0x10
            && template.u32(n_root + 0x44)? == 0x200000
            && template.u32(n_root + 0x54)? == 0x100
            && template.u64(n_root + 0x58)? == 0,
        "Native area profile template differs"
    );
    let end = [n_root + 0x60, n_root + 0x78]
        .into_iter()
        .map(|field| template.pointer(field + 8).map(|header| header - 4))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .context("Native area profile arrays")?;
    let mut out = template.0[..end].to_vec();
    let symbol = format!("{prefix}-profile-{tag:08X}");
    let mut patches = envelope_patches(&mut out, &symbol)?;
    for (field, value) in [
        (0x14, lower.to_bits()),
        (0x18, upper.to_bits()),
        (0x1C, 0),
        (0x4C, scale.to_bits()),
        (0x50, flag),
        (0x70, flags),
        (0x74, 1.0f32.to_bits()),
        (0xD0, 1.0f32.to_bits()),
        (0xB0, u32::MAX),
    ] {
        put(&mut out, n_root + field, &value.to_le_bytes())?;
    }
    out[n_root + 0x20] = u8::try_from(kind)?;
    out[n_root + 0xA0] = indicator;
    // The category mask at `+24..+4B` is the destination registry's mask of the category rows,
    // as the source's `+90..+AF` is its own registry's mask of its rows. The template's mask
    // names only its own categories. A category the registry lacks has no bit and no native filter
    // can name it, so it is left out of both and reported.
    let registry = native.tag(NATIVE_DICTIONARY, None)?;
    let held = super::perk::label::held(&registry)?;
    let (categories, absent): (Vec<u32>, Vec<u32>) =
        categories.into_iter().partition(|name| held.contains(name));
    put(
        &mut out,
        n_root + 0x24,
        &super::perk::label::mask(&categories, &registry)?,
    )?;
    let mut assets = Vec::new();
    if feedback != u32::MAX {
        let translated = feedback::translate(source, feedback, prefix)?;
        patches.push(Patch {
            offset: n_root + 0xB0,
            symbol: translated[0].symbol.clone(),
            class: 0x80803EB0,
        });
        assets.extend(translated);
    }
    let mut payload = Payload(out);
    let mut category_rows = Vec::new();
    for name in &categories {
        let mut row = vec![0u8; 0x18];
        row[..4].copy_from_slice(&name.to_le_bytes());
        row[0x10..0x14].copy_from_slice(&NATIVE_DICTIONARY.to_le_bytes());
        category_rows.extend(row);
    }
    append(
        &mut payload.0,
        n_root + 0x60,
        0x808094B3,
        &category_rows,
        0x18,
    )?;
    // Native lists the responses present as push, the fixed response and audio, in that order.
    let present =
        usize::from(push.is_some()) + usize::from(fixed.is_some()) + usize::from(audio.is_some());
    append(
        &mut payload.0,
        n_root + 0x78,
        0x80804B3D,
        &vec![0; present * 8],
        8,
    )?;
    let list = payload.array(n_root + 0x78, 8, Some(0x80804B3D))?;
    let mut objects = Vec::new();
    if let Some(push) = push {
        let at = native_sequence::object(&mut payload, 0x80804B57, 0x28)?;
        put(&mut payload.0, at, &push)?;
        objects.push(at);
    }
    if let Some(fixed) = fixed {
        let at = native_sequence::object(&mut payload, 0x80804B56, 0x0C)?;
        put(&mut payload.0, at, &fixed)?;
        objects.push(at);
    }
    if audio.is_some() {
        let audio_at = native_sequence::object(&mut payload, 0x80804B46, 0x20)?;
        let mut audio_rows = Vec::new();
        for (_, control) in &rows {
            let mut row = vec![0u8; 0x38];
            row[0x08..0x0C].copy_from_slice(&u32::MAX.to_le_bytes());
            row[0x18..0x1C].copy_from_slice(&u32::MAX.to_le_bytes());
            row[0x28..0x2C].copy_from_slice(&u32::MAX.to_le_bytes());
            row[0x30..0x34].copy_from_slice(control);
            audio_rows.extend(row);
        }
        append(&mut payload.0, audio_at, 0x80804B48, &audio_rows, 0x38)?;
        for (index, (cue, _)) in rows.iter().enumerate() {
            let at = payload.array(audio_at, 0x38, Some(0x80804B48))?[index] + 8;
            patches.push(Patch {
                offset: at,
                symbol: graph_audio::cue_symbol(*cue),
                class: NATIVE_CUE,
            });
        }
        objects.push(audio_at);
    }
    for (row, object) in list.into_iter().zip(objects) {
        native_sequence::relative(&mut payload, row, object)?;
    }
    if let Some(attribution) = attribution {
        let at = native_sequence::object(&mut payload, 0x80804B17, 0x0C)?;
        put(&mut payload.0, at, &attribution)?;
        native_sequence::relative(&mut payload, n_root + 0x58, at)?;
    }
    let length = payload.0.len() as u64;
    put(&mut payload.0, 0, &length.to_le_bytes())?;
    source.unchanged()?;
    assets.insert(
        0,
        Asset {
            symbol,
            class: 0x80809C36,
            payload,
            patches,
        },
    );
    Ok(Translated {
        assets,
        cues,
        unqualified: [p.u32(root + 0x90)?, p.u32(root + 0xA8)?],
        absent,
        silent,
    })
}

struct AudioRows {
    cues: BTreeSet<u32>,
    rows: Vec<(u32, [u8; 4])>,
    silent: Vec<u32>,
}

fn audio_rows(source: &Source, p: &Payload, audio: Option<usize>) -> Result<AudioRows> {
    // The audio response is one row-array descriptor. Its rows follow at once, after zero
    // padding whose length alignment sets, so the word before their header is the array marker
    // `80800184`.
    let mut cues = BTreeSet::new();
    let mut rows = Vec::new();
    let mut silent = Vec::new();
    let audio_rows = match audio {
        Some(audio) if p.u64(audio)? != 0 => {
            let header = p.pointer(audio + 8)?;
            ensure!(
                header >= audio + 0x14
                    && p.0
                        .get(audio + 0x10..header - 4)
                        .is_some_and(|gap| gap.iter().all(|b| *b == 0))
                    && p.u32(header - 4)? == 0x80800184,
                "Source area profile audio response header differs"
            );
            p.array(audio, 0x28, Some(0x808027C0))?
        }
        Some(audio) => {
            ensure!(
                p.u64(audio + 8)? == 0,
                "Source area profile audio response header differs"
            );
            Vec::new()
        }
        None => Vec::new(),
    };
    for row in audio_rows {
        let cue = p.u32(row + 8)?;
        ensure!(
            p.bytes::<8>(row)? == [0; 8]
                && p.bytes::<12>(row + 0x0C)? == [0; 12]
                && word(p, row + 0x18, u32::MAX)?
                && p.u32(row + 0x1C)? == 0
                && p.u32(row + 0x24)? == 0
                && source.reference(cue)? == 0x8080080A,
            "Source area profile audio row differs"
        );
        // The control word's third byte is `41` or `42`, as in `01 04 42 01`.
        let mut control = p.bytes::<4>(row + 0x20)?;
        ensure!(
            matches!(control[2], 0x41 | 0x42),
            "Source area profile audio control differs"
        );
        if control[2] == 0x42 {
            control[2] = 0x52;
        }
        // A row whose cue plays nothing has no native cue form and is left out and reported.
        if graph_audio::silent(source, cue)? {
            silent.push(cue);
            continue;
        }
        cues.insert(cue);
        rows.push((cue, control));
    }

    Ok(AudioRows { cues, rows, silent })
}

struct Responses {
    push: Option<[u8; 0x28]>,
    fixed: Option<[u8; 0x0C]>,
    audio: Option<usize>,
}

fn read_responses(p: &Payload, root: usize) -> Result<Responses> {
    // Each response appears at most once, and a profile may hold any of them or none.
    let mut push = None;
    let mut fixed = None;
    let mut audio = None;
    for row in p.array(root + 0x28, 8, Some(0x8080276E))? {
        let at = p.pointer(row)?;
        let repeated = match p.u32(at - 4)? {
            0x808023B0 => push.replace(p.bytes::<0x28>(at)?).is_some(),
            0x80802B30 => fixed.replace(p.bytes::<0x0C>(at)?).is_some(),
            0x808021B3 => audio.replace(at).is_some(),
            class => anyhow::bail!("Source area profile response {class:08X} is unqualified"),
        };
        ensure!(!repeated, "Source area profile repeats a response");
    }
    Ok(Responses { push, fixed, audio })
}

fn envelope_patches(out: &mut [u8], symbol: &str) -> Result<Vec<Patch>> {
    let mut patches = Vec::new();
    // Self identities in the envelope.
    for at in (0..out.len()).step_by(4) {
        if u32::from_le_bytes(out[at..at + 4].try_into()?) == TEMPLATE {
            put(out, at, &u32::MAX.to_le_bytes())?;
            patches.push(Patch {
                offset: at,
                symbol: symbol.to_owned(),
                class: 0x80809C36,
            });
        }
    }
    Ok(patches)
}
