//! Literal property bodies preserve D1 values under a bank-local native handler witness.
use super::*;

/// The melee flag word's bits sit one place lower in Shadowkeep. The charge test reads source
/// bit 10 (`004846CE`) where native reads bit 9 (`00B97F92`), and the other tested bits line up
/// the same way (source 6, 11 and 12 against native 5, 10 and 11). Native Scorch stores `0x0300`
/// for the source's `0x0600`. A bit without a known counterpart refuses the row.
fn flag_word(source: u32) -> Result<u32> {
    const BITS: [(u32, u32); 5] = [(6, 5), (9, 8), (10, 9), (11, 10), (12, 11)];
    ensure!(
        source <= 0xFFFF,
        "Source melee flag word exceeds its UInt16"
    );
    let mut native = 0;
    let mut left = source;
    for (from, to) in BITS {
        if source & (1 << from) != 0 {
            native |= 1 << to;
            left &= !(1 << from);
        }
    }
    ensure!(
        left == 0,
        "Source melee flag word {source:04X} sets bits without a native counterpart"
    );
    Ok(native)
}

/// Whether Shadowkeep's HUD glyph table has a row for `key`, so a HUD name the row writes draws.
fn has_glyph(reader: &mut Reader, key: u32) -> Result<bool> {
    let table = reader.tag(sundial::package_authoring::ability_hud::GLYPH_TABLE, None)?;
    let count = usize::try_from(table.u64(8)?)?;
    for index in 0..count {
        if table.u32(48 + index * 112)? == key {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn lower(
    reader: &mut Reader,
    p: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let at = p.pointer(row + 32)?;
    let source_class = p.u32(at - 4)?;
    let (class, size) = match source_class {
        // Melee integer fields: the source common handler (`0054F4A0`) and the native one
        // (`00FC01F0`) perform the same operation on the same recipient field. The flag word is
        // a UInt16 replace, the HUD-hiding field a byte replace and the HUD name a key replace.
        0x8080241C => (0x80804536, 4),
        0x8080206A => (0x80804530, 4),
        0x80802650 => (0x80804532, 4),
        0x808027CB => (0x80804509, 4),
        // A contact effect: both Float32 and the attached entity go to a list capped at three
        // (source `010E12FD`, native `0105C7AB`).
        0x80802000 => (0x8080442A, 24),
        // These records gained the native reciprocal 16-byte header. Their literal
        // bodies keep the source field order. No pointer, script or resource is copied.
        0x8080251A => (0x80804399, 32),
        0x808028BA => (0x8080442E, 8),
        0x808030A9 => (0x80804528, 4),
        0x80802591 => (0x8080411F, 4),
        // Both original handlers take the signed maximum with the current jump count.
        0x8080209C => (0x808042A5, 4),
        // The movement handlers copy this scalar to their turning recipient at +28.
        0x80802365 => (0x8080411B, 4),
        // The movement handlers raise their sprint-speed scalar to this minimum.
        0x80802B72 => (0x80804127, 4),
        // Positive enabled timings have the same write semantics. The shared writer
        // emits the target's additional empty condition list as a reciprocal pair.
        0x808028B8 => (0x80804125, 12),
        // The source callback returns a fixed factor. The native consumer instead
        // evaluates a value program, so its writer adds a constant unfiltered program.
        0x808026D1 => (0x80804520, 8),
        _ => anyhow::bail!("Unqualified source literal ability property"),
    };
    let key = p.u32(row)?;
    if class == 0x80804127 {
        ensure!(
            p.u32(p.pointer(24)? - 4)? == 0x808029B5
                && native.u32(native.pointer(24)? - 4)? == 0x80804113,
            "Sprint-speed property requires corresponding movement banks"
        );
    }
    let selection = if matches!(class, 0x808042A5 | 0x80804125 | 0x80804520) {
        let required_key = if p.u64(row + 24)? == 0 {
            0
        } else {
            let gate = p.pointer(row + 24)?;
            ensure!(
                p.u32(gate.checked_sub(4).context("Source property gate marker")?)? == 0x80802981
                    && ![0, u32::MAX, 0x811C9DC5].contains(&p.u32(gate)?),
                "Source literal property has an unqualified key gate"
            );
            p.u32(gate)?
        };
        Some(sundial::package_authoring::ability_bank::FixedSelection {
            name: p.u32(row + 4)?,
            priority: u8::try_from(p.u32(row + 8)?)?,
            required_key,
        })
    } else {
        None
    };
    let rows = native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let compatible = |r: usize| {
        native
            .pointer(r + 48)
            .ok()
            .and_then(|m| native.u32(m + 4).ok())
            == Some(class)
            && selection.is_none_or(|s| {
                (class == 0x80804520 || native.u32(r + 20).ok() == Some(s.name))
                    && native.u32(r + 24).ok() == Some(u32::from(s.priority))
                    && native_gate(native, r).ok() == Some(s.required_key)
            })
    };
    let mut matches = rows
        .iter()
        .copied()
        .filter(|&r| {
            (if class == 0x80804127 {
                // The movement property key changed between builds. A unique same-value
                // scalar in this bank witnesses its handler, not an entire native perk.
                native
                    .pointer(r + 48)
                    .ok()
                    .and_then(|m| native.u32(m + 16).ok())
                    == p.u32(at).ok()
            } else {
                native.u32(r + 16).ok() == Some(key)
            }) && compatible(r)
        })
        .collect::<Vec<_>>();
    if matches.is_empty() && class == 0x80804520 {
        // A property's key selects a row, not the typed protection callback. A
        // unique same-class row in this corresponding bank supplies its local
        // priority slot even when the original property key changed. The writer
        // replaces the duration, program and filter on a new private modifier.
        matches = rows
            .into_iter()
            .filter(|&r| compatible(r) && native.u32(r + 32).ok() == p.u32(row + 16).ok())
            .collect();
    }
    ensure!(
        matches.len() == 1,
        "Source literal property has no unique native handler witness"
    );
    let n = matches[0];
    let template_key = native.u32(n + 16)?;
    if class == 0x80804520 {
        ensure!(
            p.u32(row + 8)? == 255 && p.u64(row + 24)? == 0 && p.u32(at)? == (-1.0f32).to_bits(),
            "Ability protection requires an ungated source factor for the full activation"
        );
    }
    ensure!(
        native.u32(n + 24)? == p.u32(row + 8)?
            && native.u32(n + 32)? == p.u32(row + 16)?
            && native.u32(n + 36)? == 0
            && (selection.is_some() || native.u64(n + 40)? == 0)
            && native.u32(native.pointer(n + 48)? + 4)? == class,
        "Source literal property has another native mode, gate or class"
    );
    let mut words = [0; 8];
    // Priority indices are local to each bank. The qualified native row supplies
    // the priority slot when this private row is appended.
    match source_class {
        0x8080241C => words[0] = flag_word(u32::from(p.u16(at)?))?,
        0x8080206A => words[0] = u32::from(p.u8(at)?),
        0x80802650 => {
            words[0] = p.u32(at)?;
            ensure!(
                has_glyph(reader, words[0])?,
                "Source HUD name {:08X} has no native glyph row",
                words[0]
            );
        }
        0x80802000 => {
            ensure!(
                p.u64(at + 8)? == 0 && p.u32(at + 20)? == 0,
                "Source contact effect has additional fields"
            );
            // The source entity is a source graph. Until it translates, the hit attaches the
            // native witness's entity, which the report records beside it.
            let entity = native.u32(native.pointer(n + 48)? + 32)?;
            words[..6].copy_from_slice(&[p.u32(at)?, p.u32(at + 4)?, 0, 0, entity, 0]);
        }
        _ => {
            for (i, word) in words[..size / 4].iter_mut().enumerate() {
                *word = p.u32(at + i * 4)?;
            }
        }
    }
    let value = sundial::package_authoring::ability_bank::FixedModifier {
        template_key,
        class,
        words: sundial::package_authoring::ability_bank::widened(words),
        selection,
    };
    value.check_bank(&native.0).map_err(anyhow::Error::msg)?;
    Ok(vec![(
        AbilityRowModifier::Fixed {
            template_key,
            class,
            words,
            selection,
        },
        None,
    )])
}

fn native_gate(p: &Payload, row: usize) -> Result<u32> {
    if p.u64(row + 40)? == 0 {
        return Ok(0);
    }
    let at = p.pointer(row + 40)?;
    ensure!(
        p.u32(at.checked_sub(4).context("Native property gate marker")?)? == 0x8080454A
            && p.u32(at + 4)? == 0,
        "Native literal property has an unqualified key gate"
    );
    p.u32(at)
}
