//! Imported gear reads the native item's live dye scopes, including their animation.
use super::*;

fn accesses(text: &str, slot: usize, function: &str) -> Result<String> {
    let marker = format!("cb{slot}[");
    let mut rest = text;
    let mut result = String::new();
    while let Some(start) = rest.find(&marker) {
        result.push_str(&rest[..start]);
        let expression = &rest[start + marker.len()..];
        let mut depth = 1usize;
        let end = expression
            .char_indices()
            .find_map(|(at, c)| {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(at)
            })
            .context("Unclosed source dye index")?;
        result.push_str(&format!("{function}({})", &expression[..end]));
        rest = &expression[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

pub(in crate::d2_mot::native::effects) fn runtime(
    text: &str,
    inputs: &BTreeMap<usize, Option<usize>>,
    root: &Path,
) -> Result<(String, u64, BTreeSet<u32>)> {
    let context = load(&root.join("tfx-native/context.json"))?;
    let mut text = text.to_owned();
    for &slot in inputs.keys() {
        let count = cb_count(&text, slot)?.context("Missing source dye bank")?;
        text = replace_once(&text, &cb_decl(slot, count), "")?;
    }
    let mut slots = Vec::new();
    let mut mask = 0u64;
    let mut declarations = String::new();
    for bank in 0..3 {
        let scope = context["scopes"]
            .as_array()
            .context("Native renderer scopes")?
            .iter()
            .find(|s| s["name"] == format!("gear_dye_{bank}"))
            .context("Native gear dye scope missing")?;
        let index = scope["index"]
            .as_u64()
            .filter(|i| *i < 32)
            .context("Native dye scope mask")?;
        let tag = scope["tag"].as_str().context("Native dye scope tag")?;
        let payload = Payload(fs::read(root.join(format!("tfx-native/raw/{tag}.bin")))?);
        ensure!(
            payload.array(0x88, 16, Some(0x80800090))?.len() == 27,
            "Native dye bank has an unsupported layout"
        );
        let slot = payload.u32(0xB8)? as usize;
        ensure!(
            slot < 14 && cb_count(&text, slot)?.is_none() && !slots.contains(&slot),
            "Native dye bank conflicts with another source constant buffer"
        );
        mask |= 1 << index;
        slots.push(slot);
        declarations.push_str(&cb_decl(slot, 27));
        declarations.push('\n');
    }
    let mapping = crate::d2_mot::dye_bundle::VECTORS
        .iter()
        .map(|(_, target)| target.to_string())
        .collect::<Vec<_>>()
        .join(",");
    declarations.push_str(&format!(
        "static const uint source_dye_vectors[21] = {{{mapping}}};\n"
    ));
    for (&slot, bank) in inputs {
        let name = format!("source_dye_bank_{slot}");
        text = accesses(&text, slot, &name)?;
        if let Some(bank) = bank {
            let target = slots.get(*bank).context("Source dye bank index")?;
            declarations.push_str(&format!(
                "float4 {name}(uint i) {{ return cb{target}[source_dye_vectors[i]]; }}\n"
            ));
        } else {
            declarations.push_str(&format!(
                "float4 {name}(uint i) {{ uint bank = i < 9 ? i / 3 : (i - 9) / 18; uint local = i < 9 ? i % 3 : 3 + (i - 9) % 18; uint vector_index = source_dye_vectors[local]; if (bank == 0) return cb{}[vector_index]; if (bank == 1) return cb{}[vector_index]; return cb{}[vector_index]; }}\n",
                slots[0], slots[1], slots[2]));
        }
    }
    Ok((
        format!("{declarations}\n{text}"),
        mask,
        slots.into_iter().map(|slot| slot as u32).collect(),
    ))
}

/// Native dye textures occupy 3..8. Keep the source dye map in the vacated slot 9.
pub(in crate::d2_mot::native::effects) const fn texture_slot(source: u32) -> u32 {
    match source {
        3 => 9,
        4..=9 => source - 1,
        _ => source,
    }
}

pub(in crate::d2_mot::native::effects) fn textures(text: &str) -> String {
    let mut text = text.to_owned();
    // Placeholders avoid cascading t4 -> t3 -> t9 replacements.
    for slot in 3..=9 {
        text = text.replace(
            &format!("t{slot} : register(t{slot})"),
            &format!(
                "source_dye_texture_{slot} : register(t{})",
                texture_slot(slot)
            ),
        );
        text = text.replace(&format!("t{slot}."), &format!("source_dye_texture_{slot}."));
    }
    text
}
