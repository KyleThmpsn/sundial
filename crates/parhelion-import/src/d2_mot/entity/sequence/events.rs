//! Event objects, linked only to completed native dependencies.
use super::{Bindings, Sequence, controls::*, expression};
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

fn delay(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    ensure!(
        source.u32(from.checked_sub(4).context("delay source class offset")?)? == 0x808091D7,
        "unsupported source delay class"
    );
    ensure!(source.u32(from + 28)? == 0, "delay event kind differs");
    ensure!(
        output.u32(to.checked_sub(4).context("delay native class offset")?)? == 0x808093CB,
        "native delay class differs"
    );
    common(source, from, output, to)?;
    // The common source base ends with kind at +1C. This delay class has no
    // selector at +20, which can already belong to the following object.
    put(output, to + 0x2C, &[0; 12])?;
    Ok(())
}

pub(super) fn link(
    bindings: &Bindings,
    offset: usize,
    source_class: u32,
    target_class: u32,
) -> Result<u32> {
    let dependency = bindings
        .resources
        .get(&offset)
        .with_context(|| format!("untranslated sequence resource at {offset:X}"))?;
    ensure!(
        dependency.source_class == source_class && dependency.native_class == target_class,
        "sequence dependency class differs at {offset:X}"
    );
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&dependency.tag),
        "sequence dependency tag is invalid"
    );
    Ok(dependency.tag)
}

fn render(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
) -> Result<()> {
    ensure!(
        source.u32(from.checked_sub(4).context("render source class offset")?)? == 0x8080694C,
        "unsupported source render event class"
    );
    ensure!(source.u32(from + 0x1C)? == 34, "render event kind differs");
    ensure!(
        output.u32(to.checked_sub(4).context("render native class offset")?)? == 0x80806E51,
        "native render event class differs"
    );
    ensure!(
        source.0.get(from + 0x28..from + 0x100) == Some(&[0u8; 216][..]),
        "render event expressions require translation"
    );
    let flags = source.u32(from + 0x24)?;
    ensure!(
        matches!(flags, 0 | 1 | 2 | 3 | 4 | 7),
        "unsupported render event flags"
    );
    let resource = source.u32(from + 0x20)?;
    let target = if resource == u32::MAX {
        u32::MAX
    } else {
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&resource),
            "invalid source render resource tag"
        );
        link(bindings, from + 0x20, 0x8080694E, 0x80806E53)?
    };
    // This supported form has no transform or renderer expressions. Clear its
    // entire native body instead of inheriting a template's authored settings.
    put(output, to + 16, &[0; 264])?;
    common(source, from, output, to)?;
    put(output, to + 0x30, &34u32.to_le_bytes())?;
    put(output, to + 0x38, &target.to_le_bytes())?;
    put(output, to + 0x3C, &flags.to_le_bytes())?;
    Ok(())
}

fn table(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    locators: &BTreeMap<u32, u32>,
    locator_count: usize,
) -> Result<()> {
    ensure!(
        source.u32(
            from.checked_sub(4)
                .context("table event source class offset")?
        )? == 0x80803F1F,
        "unsupported source table event class"
    );
    ensure!(
        output.u32(
            to.checked_sub(4)
                .context("table event native class offset")?
        )? == 0x80804AB4,
        "native table event class differs"
    );
    ensure!(
        source.u32(from + 0x1C)? == 30 && source.u32(from + 0x20)? == 0xFF,
        "table event kind or selector differs"
    );
    ensure!(
        source.u64(from + 0x28)? == 0,
        "table event reserved fields differ"
    );
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&source.u32(from + 0x30)?)
            && source.u32(from + 0x34)? == 1
            && source.u64(from + 0x38)? == 0,
        "table event reference encoding requires translation"
    );
    ensure!(
        source.bytes::<16>(from + 0x40)? == [0; 16],
        "second table event resource requires translation"
    );
    ensure!(
        source.bytes::<16>(from + 0x50)? == [0; 16],
        "table event parameter extension requires translation"
    );
    let lower = source.f32(from + 0x60)?;
    let upper = source.f32(from + 0x64)?;
    ensure!(
        lower.is_finite() && upper.is_finite() && lower <= upper,
        "invalid table event bounds"
    );
    let flags = source.bytes::<4>(from + 0x6C)?;
    ensure!(
        flags[..3].iter().all(|&flag| flag <= 1) && flags[3] == 0,
        "unsupported table event flags"
    );
    ensure!(
        source.u64(from + 0x70)? == 0 && source.u64(from + 0x78)? == u64::MAX,
        "table event source extension requires translation"
    );
    let source_names = source.array(source.pointer(24)? + 0x250, 4, Some(0x80809538))?;
    let attachment = source.u32(from + 0x68)?;
    ensure!(
        (attachment as usize) < source_names.len(),
        "table event locator outside source declaration"
    );
    let locator = *locators
        .get(&attachment)
        .context("unmapped table event attachment")?;
    ensure!(
        (locator as usize) < locator_count,
        "table event locator outside native declaration"
    );
    let resource = link(bindings, from + 0x30, 0x8080873F, 0x80808BCD)?;
    put(output, to + 16, &[0; 112])?;
    common(source, from, output, to)?;
    put(output, to + 0x30, &30u32.to_le_bytes())?;
    put(output, to + 0x38, &0xFFu32.to_le_bytes())?;
    put(output, to + 0x40, &source.u32(from + 0x24)?.to_le_bytes())?;
    put(output, to + 0x50, &u64::from(resource).to_le_bytes())?;
    put(output, to + 0x70, &source.bytes::<8>(from + 0x60)?)?;
    put(output, to + 0x78, &locator.to_le_bytes())?;
    put(output, to + 0x7C, &flags)?;
    Ok(())
}

fn particle(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    locators: usize,
) -> Result<()> {
    ensure!(
        source.u32(from + 0x24)? == 0,
        "particle event padding differs"
    );
    let rows = source.array(from + 0x28, 24, Some(0x808067BB))?;
    ensure!(!rows.is_empty(), "particle event has no systems");
    let start = array(output, to + 0x40, 0x80806CC8, &vec![0; rows.len() * 24], 24)?;
    for (index, &row) in rows.iter().enumerate() {
        let target = start + index * 24;
        let mut parameters = Vec::new();
        for at in source.array(row, 1, Some(0x80800009))? {
            let value = source.u8(at)?;
            ensure!(
                (value as usize) < locators,
                "particle locator outside sequence"
            );
            parameters.push(value);
        }
        array(output, target, 0x80800009, &parameters, 1)?;
        super::presentation::fields(source, row)?;
        let tag = if source.u32(row + 16)? == u32::MAX {
            u32::MAX
        } else {
            link(bindings, row + 16, 0x80806920, 0x80806E28)?
        };
        put(output, target + 16, &tag.to_le_bytes())?;
        put(output, target + 20, &source.bytes::<4>(row + 20)?)?;
    }
    for delta in [0, 72, 144] {
        expression::write(source, from + 0x38 + delta, output, to + 0x50 + delta)?;
    }
    let label = source.u32(from + 0x108)?;
    let mapped = bindings
        .identities
        .get(&label)
        .with_context(|| format!("untranslated particle event identity {label:08X}"))?;
    put(output, to + 0x120, &mapped.to_le_bytes())?;
    ensure!(
        source.u32(from + 0x10C)? == 0,
        "particle event trailing padding differs"
    );
    Ok(())
}

fn spawn(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    locators: &BTreeMap<u32, u32>,
    locator_count: usize,
) -> Result<()> {
    ensure!(
        source.0.get(from..from + 0x110).is_some(),
        "spawn source extent"
    );
    ensure!(output.0.get(to..to + 0xE0).is_some(), "spawn native extent");
    ensure!(
        source.u32(from.checked_sub(4).context("spawn source class offset")?)? == 0x80808485,
        "unsupported source spawn class"
    );
    ensure!(
        output.u32(to.checked_sub(4).context("spawn native class offset")?)? == 0x80808881,
        "native spawn class differs"
    );
    ensure!(
        source.u32(from + 0x1C)? == 16 && source.u32(from + 0x20)? == 0xFF,
        "spawn event kind or selector differs"
    );
    ensure!(
        source.u32(from + 0x24)? == 0 && source.bytes::<16>(from + 0x28)? == [0; 16],
        "spawn reference extension requires translation"
    );
    for delta in (0x38..0x60).step_by(4) {
        ensure!(
            source.f32(from + delta)?.is_finite(),
            "nonfinite spawn pose"
        );
    }
    let request =
        u16::try_from(source.u32(from + 0x60)?).context("spawn request exceeds native width")?;
    let mode = source.u32(from + 0x64)?;
    ensure!(
        mode & 0x00FFFFFF == 0 && mode >> 24 <= 2,
        "unsupported spawn mode"
    );
    let attachment = source.u32(from + 0x68)?;
    ensure!(
        attachment <= 0xFF && source.u32(from + 0x6C)? == 0,
        "spawn attachment encoding differs"
    );
    let attachment = if attachment == 0xFF {
        0xFF
    } else {
        let names = source.array(source.pointer(24)? + 0x250, 4, Some(0x80809538))?;
        ensure!(
            (attachment as usize) < names.len(),
            "spawn locator outside source declaration"
        );
        let mapped = *locators
            .get(&attachment)
            .context("unmapped spawn attachment")?;
        ensure!(
            (mapped as usize) < locator_count && mapped < 0xFF,
            "spawn locator outside native declaration"
        );
        mapped as u8
    };
    let tag = source.u32(from + 0x70)?;
    let form = source.u32(from + 0x74)?;
    let hash = source.u64(from + 0x78)?;
    ensure!(
        ((0x80800001..=0x81FFFFFF).contains(&tag) && form == 1 && hash == 0)
            || (tag == u32::MAX && form == 0 && hash != 0),
        "spawn entity reference encoding requires translation"
    );
    ensure!(
        source.bytes::<48>(from + 0x80)? == [0; 48],
        "spawn arrays require translation"
    );
    for delta in (0xB4..0xC8).step_by(4) {
        ensure!(
            source.f32(from + delta)?.is_finite(),
            "nonfinite spawn parameter"
        );
    }
    ensure!(
        source.u64(from + 0xC8)? == 0
            && source.u32(from + 0xD0)? <= 1
            && source.bytes::<36>(from + 0xD4)? == [0; 36],
        "spawn parameter extension requires translation"
    );
    ensure!(
        source.u64(from + 0xF8)? == 0
            && [0x100, 0x104, 0x108].iter().all(|&delta| source
                .u32(from + delta)
                .is_ok_and(|name| name == 0x811C9DC5))
            && source.u32(from + 0x10C)? == 0,
        "spawn source callbacks require translation"
    );
    let entity = link(bindings, from + 0x70, 0x80809AD8, 0x80809C0F)?;
    put(output, to + 16, &[0; 208])?;
    common(source, from, output, to)?;
    put(output, to + 0x30, &15u32.to_le_bytes())?;
    put(output, to + 0x38, &0xFFu32.to_le_bytes())?;
    put(output, to + 0x48, &source.bytes::<40>(from + 0x38)?)?;
    put(output, to + 0x70, &request.to_le_bytes())?;
    put(output, to + 0x72, &[(mode >> 24) as u8, attachment])?;
    put(output, to + 0x74, &entity.to_le_bytes())?;
    put(output, to + 0x98, &source.bytes::<72>(from + 0xB0)?)?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn sound(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    sequence: &Sequence,
    inputs: &BTreeMap<u32, u32>,
    locators: usize,
) -> Result<()> {
    ensure!(
        source.bytes::<20>(from + 0x24)? == [0; 20]
            && source.u64(from + 0x48)? == 1
            && source.u64(from + 0x60)? == 0,
        "sound event contains unsupported switch expressions"
    );
    put(
        output,
        to + 0x40,
        &link(bindings, from + 0x50, 0x80809738, 0x80809802)?.to_le_bytes(),
    )?;
    let mut parameters = Vec::new();
    for group in source.array(from + 0x38, 72, Some(0x8080B6DE))? {
        ensure!(
            source.bytes::<48>(group)? == [0; 48] && source.u32(group + 52)? == 0,
            "sound parameter expression requires translation"
        );
        let name = source.u32(group + 48)?;
        let indices = source.array(group + 56, 4, Some(0x80800007))?;
        ensure!(
            indices.len() == 1,
            "sound parameter group requires expression lowering"
        );
        let index = source.u32(indices[0])?;
        ensure!(
            sequence
                .inputs
                .get(index as usize)
                .is_some_and(|input| input.name == name),
            "sound parameter name does not match its sequence input"
        );
        parameters.extend(
            inputs
                .get(&index)
                .context("unmapped sound parameter")?
                .to_le_bytes(),
        );
    }
    ensure!(
        parameters.len() <= 6 * 4,
        "native sound parameter capacity exceeded"
    );
    array(output, to + 0x48, 0x80800007, &parameters, 4)?;
    let locator = source.u8(from + 0x68)?;
    ensure!(
        (locator as usize) < locators && source.bytes::<7>(from + 0x69)? == [0; 7],
        "sound attachment locator is outside the sequence"
    );
    // Native 010E6E40 indexes the locator table with this byte. It is not flags.
    put(output, to + 0x70, &[locator])?;
    Ok(())
}

pub(super) fn write(
    source: &Payload,
    sequence: &Sequence,
    template: &Payload,
    output: &mut Payload,
    bindings: &Bindings,
    inputs: &BTreeMap<u32, u32>,
    locators: usize,
) -> Result<Allocation> {
    let root = output.pointer(16)?;
    let definition = output.pointer(24)?;
    let owner = output.u32(root)?;
    let source_rows = source.array(source.pointer(24)? + 0x1D8, 24, Some(0x808091F1))?;
    let count = source_rows.len();
    ensure!(count <= i16::MAX as usize, "too many sequence events");
    let mut envelopes = BTreeMap::new();
    for row in template.array(template.pointer(24)? + 0x168, 24, Some(0x808093E6))? {
        let node = template.pointer(row + 16)?;
        let class = template.u32(node - 4)?;
        let instance = usize::try_from(template.u64(node + 8)?)?;
        envelopes.entry(class).or_insert(instance);
    }
    let ir = array(output, root + 0xB0, 0x808093E5, &vec![0; count * 48], 48)?;
    let dr = array(
        output,
        definition + 0x168,
        0x808093E6,
        &vec![0; count * 24],
        24,
    )?;
    let mut allocation = Allocation::array(0xE3CC5C98, 0x808093E5, count);
    for (index, &row) in source_rows.iter().enumerate() {
        let from = source.pointer(row + 16)?;
        let class = source.u32(from - 4)?;
        let (dc, ic, size, isize, kind) = match class {
            0x808091D7 => (0x808093CB, 0x808093CA, 0x38, 0x40, 0),
            0x808067B9 => (0x80806CC6, 0x80806CC5, 0x128, 0x50, 4),
            0x80806640 => (0x80806B38, 0x80806B37, 0x78, 0x60, 6),
            0x8080694C => (0x80806E51, 0x80806E50, 0x118, 0x50, 34),
            0x80803F1F => (0x80804AB4, 0x80804AB3, 0x80, 0x40, 30),
            0x80808485 => (0x80808881, 0x80808880, 0xE0, 0x70, 15),
            0x80806A48 => (0x80806F40, 0x80806F3F, 0x130, 0x90, 39),
            0x80806A52 => (0x80806F49, 0x80806F48, 0x150, 0xB0, 33),
            _ => anyhow::bail!("unsupported sequence event {class:08X}"),
        };
        let source_kind = if class == 0x80808485 { 16 } else { kind };
        ensure!(
            source.u32(from + 0x1C)? == source_kind
                && (matches!(kind, 0 | 34) || source.u32(from + 0x20)? == 0xFF),
            "sequence event kind or selector differs"
        );
        let native_instance = *envelopes.get(&dc).context("native event envelope absent")?;
        ensure!(
            template.u32(native_instance - 4)? == ic,
            "native event instance class differs"
        );
        let instance_row = ir + index * 48;
        let definition_row = dr + index * 24;
        pair(
            output,
            owner,
            instance_row,
            definition_row,
            0x808093E5,
            0x808093E6,
        )?;
        relative(output, instance_row + 16, root)?;
        let instance = object(output, ic, isize);
        let target = object(output, dc, size);
        let defaults = template
            .0
            .get(native_instance..native_instance + isize)
            .context("native event instance extent")?;
        if class == 0x80808485 {
            ensure!(
                defaults[24..80] == [0; 56]
                    && defaults[80..96] == [0xFF; 16]
                    && defaults[96..112] == [0; 16],
                "native spawn runtime defaults differ"
            );
        }
        put(output, instance, defaults)?;
        pair(output, owner, instance, target, ic, dc)?;
        relative(output, instance + 16, instance_row)?;
        relative(output, instance_row + 32, instance)?;
        relative(output, definition_row + 16, target)?;
        if kind == 0 {
            delay(source, from, output, target)?;
        } else if kind == 34 {
            render(source, from, output, target, bindings)?;
        } else if kind == 30 {
            // Assembly copies source locator declarations in their original
            // order. Record conversion still requires an explicit index map.
            let attachment = source.u32(from + 0x68)?;
            table(
                source,
                from,
                output,
                target,
                bindings,
                &BTreeMap::from([(attachment, attachment)]),
                locators,
            )?;
        } else if class == 0x80808485 {
            let attachment = source.u32(from + 0x68)?;
            spawn(
                source,
                from,
                output,
                target,
                bindings,
                &BTreeMap::from([(attachment, attachment)]),
                locators,
            )?;
        } else {
            common(source, from, output, target)?;
            put(output, target + 0x30, &kind.to_le_bytes())?;
            put(output, target + 0x38, &0xFFu32.to_le_bytes())?;
        }
        match kind {
            0 | 15 | 30 | 34 => {}
            4 => particle(source, from, output, target, bindings, locators)?,
            6 => sound(
                source, from, output, target, bindings, sequence, inputs, locators,
            )?,
            33 | 39 => {
                ensure!(
                    source.u32(from + 0x28)? == 2 && source.u32(from + 0x2C)? == 0,
                    "child entity event reference mode differs"
                );
                put(
                    output,
                    target + 0x40,
                    &link(bindings, from + 0x24, 0x80809AD8, 0x80809C0F)?.to_le_bytes(),
                )?;
                let start = if kind == 33 { 0x58 } else { 0x40 };
                let end = if kind == 33 { 0x78 } else { 0x60 };
                put(
                    output,
                    target + 0x50,
                    source
                        .0
                        .get(from + 0x30..from + start)
                        .context("child entity event parameters")?,
                )?;
                for delta in [0, 72, 144] {
                    expression::write(source, from + start + delta, output, target + end + delta)?;
                }
            }
            _ => unreachable!(),
        }
        allocation.children.push(Allocation::node(ic));
    }
    array(output, root + 0x100, 0x8080000A, &vec![0xFF; count * 2], 2)?;
    put(
        output,
        definition + 0x19C,
        &u32::try_from(count)?.to_le_bytes(),
    )?;
    Ok(allocation)
}
