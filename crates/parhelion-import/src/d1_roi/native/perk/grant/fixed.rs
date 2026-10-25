//! Literal property bodies preserve D1 values under a bank-local native handler witness.
use super::*;

pub(super) fn lower(
    p: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let at = p.pointer(row + 32)?;
    let (class, size) = match p.u32(at - 4)? {
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
    let selection = if matches!(class, 0x808042A5 | 0x80804125) {
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
    let matches = native
        .array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?
        .into_iter()
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
            }) && native
                .pointer(r + 48)
                .ok()
                .and_then(|m| native.u32(m + 4).ok())
                == Some(class)
                && selection.is_none_or(|s| {
                    native.u32(r + 20).ok() == Some(s.name)
                        && native.u32(r + 24).ok() == Some(u32::from(s.priority))
                        && native_gate(native, r).ok() == Some(s.required_key)
                })
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Source literal property has no unique native handler witness"
    );
    let n = matches[0];
    let template_key = native.u32(n + 16)?;
    ensure!(
        native.u32(n + 24)? == p.u32(row + 8)?
            && native.u32(n + 32)? == p.u32(row + 16)?
            && native.u32(n + 36)? == 0
            && (selection.is_some() || native.u64(n + 40)? == 0)
            && native.u32(native.pointer(n + 48)? + 4)? == class,
        "Source literal property has another native mode, gate or class"
    );
    let mut words = [0; 8];
    // Handler indices are local to each bank. The qualified native row supplies
    // the handler when this private row is appended.
    for (i, word) in words[..size / 4].iter_mut().enumerate() {
        *word = p.u32(at + i * 4)?;
    }
    let value = sundial::package_authoring::ability_bank::FixedModifier {
        template_key,
        class,
        words,
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
