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
        _ => anyhow::bail!("Unqualified source literal ability property"),
    };
    let key = p.u32(row)?;
    let matches = native
        .array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?
        .into_iter()
        .filter(|&r| {
            native.u32(r + 16).ok() == Some(key)
                && native
                    .pointer(r + 48)
                    .ok()
                    .and_then(|m| native.u32(m + 4).ok())
                    == Some(class)
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Source literal property has no unique matching native key"
    );
    let n = matches[0];
    ensure!(
        native.u32(n + 24)? == 255
            && native.u32(n + 32)? == p.u32(row + 16)?
            && native.u32(n + 36)? == 0
            && native.u64(n + 40)? == 0
            && native.u32(native.pointer(n + 48)? + 4)? == class,
        "Source literal property has another native mode, gate or class"
    );
    let mut words = [0; 8];
    // Handler indices are local to each bank. The native same-key, same-class
    // row supplies the handler when this private row is appended.
    for (i, word) in words[..size / 4].iter_mut().enumerate() {
        *word = p.u32(at + i * 4)?;
    }
    let value = sundial::package_authoring::ability_bank::FixedModifier {
        template_key: key,
        class,
        words,
    };
    value.check_bank(&native.0).map_err(anyhow::Error::msg)?;
    Ok(vec![(
        AbilityRowModifier::Fixed {
            template_key: key,
            class,
            words,
        },
        None,
    )])
}
