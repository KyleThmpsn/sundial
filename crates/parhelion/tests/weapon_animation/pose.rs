//! Observe profile pose weights through the emitted controller's numeric declarations.
use super::*;

#[allow(clippy::cognitive_complexity)]
fn weights(
    reader: &mut Reader,
    rig: &Value,
    modern: bool,
    required: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, BTreeSet<u32>>> {
    let find = |class: &str, fp: bool| -> Result<u32> {
        let rows = rig["components"]
            .as_array()
            .context("Components")?
            .iter()
            .filter(|c| c["class"] == class && (c["entity"] != rig["runtime_entity"]) == fp)
            .collect::<Vec<_>>();
        ensure!(rows.len() == 1, "Pose owner is ambiguous");
        number(&rows[0]["owner"])
    };
    let attachment = reader.tag(
        find(if modern { "8080356E" } else { "80804221" }, false)?,
        None,
    )?;
    let base = attachment.pointer(24)?;
    let field = base + if modern { 0x680 } else { 0x588 };
    let stride = if modern { 0x128 } else { 0xE8 };
    let content = number(&rig["content_key"])?;
    let selected = attachment
        .array(field, stride, None)?
        .into_iter()
        .filter(|&r| {
            attachment.u32(r).ok() == Some(content)
                || (modern && attachment.u32(r + 0x28).ok() == Some(content))
        })
        .collect::<Vec<_>>();
    ensure!(selected.len() <= 1, "Pose attachment is ambiguous");
    let row = selected.first().copied().unwrap_or(field - stride);
    let profile = attachment.u32(row + if modern { 0x94 } else { 0x6C })?;
    let owner = reader.tag(
        find(if modern { "8080263C" } else { "80803640" }, true)?,
        None,
    )?;
    let base = owner.pointer(24)?;
    let inputs = owner.array(base + if modern { 0xC8 } else { 0xA8 }, 40, None)?;
    let layers = reader.tag(owner.u32(base + if modern { 0x14C } else { 0x11C })?, None)?;
    let mut result = BTreeMap::<u32, BTreeSet<u32>>::new();
    for group in layers.array(8, if modern { 32 } else { 24 }, None)? {
        let name = layers.u32(group + 16)?;
        if !required.contains(&name) {
            continue;
        }
        for choice in layers.array(group, 16, None)? {
            for option in layers.array(choice, if modern { 80 } else { 64 }, None)? {
                let conditions = layers.array(option + if modern { 0 } else { 16 }, 8, None)?;
                if modern
                    && !conditions.iter().any(|&r| {
                        layers.u32(r).ok() == Some(3) && layers.u32(r + 4).ok() == Some(profile)
                    })
                {
                    continue;
                }
                if layers.u32(option + 32)? == 2 && layers.u16(option + 42)? == 0 {
                    let input = *inputs
                        .get(usize::from(layers.u16(option + 40)?))
                        .context("Pose input index")?;
                    result
                        .entry(name)
                        .or_default()
                        .insert(owner.u32(input + 32)?);
                }
            }
        }
    }
    Ok(result)
}

pub(super) fn check(
    source: &mut Reader,
    staged: &mut Reader,
    source_rig: &Value,
    emitted_rig: &Value,
    required: &Value,
) -> Result<Value> {
    let required = required
        .as_array()
        .context("Required pose layers")?
        .iter()
        .map(number)
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(!required.is_empty(), "Required pose layers are empty");
    let expected = weights(source, source_rig, true, &required)?;
    let actual = weights(staged, emitted_rig, false, &required)?;
    for name in required {
        let inputs = expected
            .get(&name)
            .context("Configured source layer has no live profile weights")?;
        ensure!(
            actual
                .get(&name)
                .is_some_and(|values| inputs.is_subset(values)),
            "Source profile weight input was lost for layer {name:08X}"
        );
    }
    Ok(json!({"source_input_names":expected,"emitted_input_names":actual}))
}
