//! Source values written into native status, receiver and channel bank owners.
use super::*;
use crate::{
    d1_roi::native::perk::pattern::effect::{Connected, Host, Lifetime},
    presentation::append,
    tiger::{instance, sequence::relative},
};

pub(super) fn finish(target: &mut Payload) -> Result<()> {
    let size = u64::try_from(target.0.len())?;
    put(&mut target.0, 0, &size.to_le_bytes())?;
    let end = target.0.len();
    instance::seal_span(target, &mut Vec::new(), end)?;
    let size = u64::try_from(target.0.len())?;
    put(&mut target.0, 0, &size.to_le_bytes())
}

pub(super) fn expression(target: &Payload, at: usize) -> Result<(Vec<u8>, Vec<usize>)> {
    let code = target
        .array(at + 0x10, 1, Some(0x80800009))?
        .into_iter()
        .map(|a| target.u8(a))
        .collect::<Result<Vec<_>>>()?;
    Ok((code, target.array(at + 0x20, 16, Some(0x80800090))?))
}

/// Write a connected source status into a native connected status, class `80804205`.
/// Duration `+98`, Timer flag `+120`, competition and both expressions take source values.
/// The caller resolves the source HUD identity before this writer retains its name and policy.
pub(super) fn status(target: &mut Payload, fields: &Connected) -> Result<Value> {
    let root = target.pointer(24)?;
    let h = root + 0xE0;
    let d = h + 0x148;
    let sequence = root + 0x48;
    let sequence_live = usize::try_from(target.u64(sequence + 8)?)?;
    let owner = target.u32(root)?;
    ensure!(
        target.u32(root - 4)? == 0x80804205
            && target.u32(h + 4)? == 0x80804211
            && target.u32(d + 4)? == 0x80803B32
            && target.u32(sequence)? == owner
            && target.u32(sequence + 4)? == 0x80803A60
            && target.u32(sequence_live + 4)? == 0x80803A61
            // The template's lifetime receiver policy matches the source's: zero required, as
            // in 81A69985, or two optional, as in 81A699C5 with its unbound lifetime requests.
            && target.u64(root + 0x70)? == fields.receiver_policy
            && target.u64(root + 0xC0)? == 0x3C18FBCE,
        "Native connected status envelope differs"
    );
    put(&mut target.0, root + 0x58, &fields.competition)?;
    put(&mut target.0, h + 0x80, &fields.hud)?;
    put(&mut target.0, d + 0x98, &fields.duration)?;
    put(&mut target.0, d + 0x120, &fields.timer)?;
    let (code, values) = expression(target, h + 0x98)?;
    ensure!(
        code == [0x34, 0, 0x3E, 0] && values.len() == 1,
        "Native status count is not a constant"
    );
    put(&mut target.0, values[0], &fields.count)?;
    let program = lifetime(target, owner, d + 0x128, &fields.lifetime)?;
    finish(target)?;
    Ok(
        json!({"competition":hex::encode(fields.competition),"hud":hex::encode(fields.hud),
        "duration":hex::encode(fields.duration),"timer":hex::encode(fields.timer),
        "count":hex::encode(fields.count),"lifetime":program,"hud_name_translated":true}),
    )
}

/// Write a source Duration host into a native Duration host, class `80803B30`. Duration `+98`,
/// Timer flag `+120` and the lifetime expression take source values.
pub(super) fn host(target: &mut Payload, fields: &Host) -> Result<Value> {
    let root = target.pointer(24)?;
    let d = root + 0x48;
    let owner = target.u32(root)?;
    ensure!(
        target.u32(root - 4)? == 0x80803B30
            && target.u32(d)? == owner
            && target.u32(d + 4)? == 0x80803B32,
        "Native Duration host envelope differs"
    );
    put(&mut target.0, d + 0x98, &fields.duration)?;
    put(&mut target.0, d + 0x120, &fields.timer)?;
    let program = lifetime(target, owner, d + 0x128, &fields.lifetime)?;
    finish(target)?;
    Ok(
        json!({"duration":hex::encode(fields.duration),"timer":hex::encode(fields.timer),
        "lifetime":program}),
    )
}

/// Write a source damage component (`damage_component.bs`, definition base `80802872`) into the
/// native template's (`80804B8A`, instance `80804BEE`) when both have the same form: the same
/// stat values without programs, one damage section of the same name, one shield, one routing,
/// one meter, no intercepts, regions, garbage regions, meter providers or ping regions, the same
/// accelerations and both display flags set. Fields pair by name hash, `m_` and the source name,
/// for twenty members in the same order. Only the base vitality varies among stock overshields,
/// so the source's carries to native definition `+1A8` and its instance copy `+324`.
pub(super) fn damage(target: &mut Payload, source: &Payload) -> Result<Value> {
    let s = source.pointer(24)?;
    let n = target.pointer(24)?;
    let instance = target.pointer(16)?;
    ensure!(
        target.u32(n - 4)? == 0x80804B8A && target.u32(instance - 4)? == 0x80804BEE,
        "Native damage component envelope differs"
    );
    for row in 0..11 {
        let (from, to) = (s + 0x60 + row * 0x20, n + 0x48 + row * 0x20);
        ensure!(
            source.u64(from)? == 0
                && target.u64(to)? == 0
                && source.u32(from + 0x0C)? == target.u32(to + 0x0C)?,
            "Source damage stats differ from the native component's"
        );
    }
    let count = |p: &Payload, at: usize| p.u64(at);
    let sections = source.array(s + 0x1C8, 0xD0, Some(0x808029BC))?;
    let native_sections = target.array(n + 0x1B0, 0x138, Some(0x80804C11))?;
    // Each section row opens with its reciprocal owner header. The name and its value follow at
    // source `+18/+1C` and native `+10/+14`.
    ensure!(
        sections.len() == 1
            && native_sections.len() == 1
            && source.u32(sections[0] + 0x18)? == target.u32(native_sections[0] + 0x10)?
            && source.u32(sections[0] + 0x1C)? == target.u32(native_sections[0] + 0x14)?,
        "Source damage sections differ from the native component's"
    );
    for (from, to, expected) in [
        (0x310, 0x328, 1),
        (0x320, 0x338, 0),
        (0x330, 0x348, 1),
        (0x340, 0x368, 1),
        (0x350, 0x378, 0),
        (0x360, 0x388, 0),
        (0x370, 0x398, 0),
        (0x380, 0x3A8, 0),
    ] {
        ensure!(
            count(source, s + from)? == expected && count(target, n + to)? == expected,
            "Source damage component has other routings, shields, meters or regions"
        );
    }
    ensure!(
        source.bytes::<0x18>(s + 0x500)? == target.bytes::<0x18>(n + 0x500)?
            && source.bytes::<2>(s + 0x550)? == [1, 1]
            && target.bytes::<2>(n + 0x580)? == [1, 1],
        "Source damage accelerations or display flags differ"
    );
    let vitality = source.f32(s + 0x1C0)?;
    ensure!(
        vitality.is_finite() && vitality > 0.0,
        "Source damage component has no positive base vitality"
    );
    put(&mut target.0, n + 0x1A8, &vitality.to_le_bytes())?;
    put(&mut target.0, instance + 0x324, &vitality.to_le_bytes())?;
    Ok(json!({"base_vitality":vitality,"section":source.u32(sections[0] + 0x18)?}))
}

/// Write a source lifetime into a native Duration's expression at `at`. A constant needs a
/// constant envelope without inputs, a program one named direct channel input.
pub(super) fn lifetime(
    target: &mut Payload,
    owner: u32,
    at: usize,
    lifetime: &Lifetime,
) -> Result<Value> {
    let inputs = target.array(at + 0x40, 40, Some(0x80809789))?;
    Ok(match lifetime {
        Lifetime::Constant(value) => {
            let (code, values) = expression(target, at)?;
            ensure!(
                code == [0x34, 0, 0x3E, 0] && values.len() == 1 && inputs.is_empty(),
                "Native lifetime is not a constant"
            );
            put(&mut target.0, values[0], value)?;
            json!({"constant":hex::encode(value)})
        }
        Lifetime::Program {
            code,
            constants,
            input,
        } => {
            // Both builds reserve input slot zero, so slot one is the one named input.
            let [row] = inputs.as_slice() else {
                anyhow::bail!("Native lifetime does not read one named input");
            };
            ensure!(
                target.u64(at + 0x30)? == 2
                    && target.u64(at + 0x38)? == 1
                    && target.u32(*row)? == owner
                    && target.u32(*row + 4)? == 0x80809788
                    && target.u64(*row + 0x10)? == 0
                    && target.u64(*row + 0x18)? == 0x808097C1,
                "Native lifetime input is not a direct named channel"
            );
            put(&mut target.0, *row + 0x20, &input.to_le_bytes())?;
            append(&mut target.0, at + 0x10, 0x80800009, code, 1)?;
            append(
                &mut target.0,
                at + 0x20,
                0x80800090,
                &constants.concat(),
                16,
            )?;
            json!({"code":hex::encode(code),"constants":constants.iter().map(hex::encode)
                .collect::<Vec<_>>(),"input":input})
        }
        Lifetime::Builtin { code, constants } => {
            // A program over constants and the builtin clock in slot zero needs no input row,
            // as native `005790A0` always supplies that slot. It takes the constant envelope
            // with one slot in each count and the fast path at `+3C` cleared.
            let (envelope, _) = expression(target, at)?;
            ensure!(
                envelope == [0x34, 0, 0x3E, 0] && inputs.is_empty(),
                "Native lifetime is not a constant envelope"
            );
            append(&mut target.0, at + 0x10, 0x80800009, code, 1)?;
            append(
                &mut target.0,
                at + 0x20,
                0x80800090,
                &constants.concat(),
                16,
            )?;
            put(
                &mut target.0,
                at + 0x30,
                &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
            )?;
            json!({"code":hex::encode(code),"constants":constants.iter().map(hex::encode)
                .collect::<Vec<_>>(),"builtin_clock":true})
        }
    })
}

/// Change the count of the one allocation row of `class`, following inherited and child rows.
fn recount(p: &mut Payload, class: u32, before: usize, after: usize) -> Result<usize> {
    fn visit(
        p: &mut Payload,
        row: usize,
        class: u32,
        counts: (usize, usize),
        seen: &mut BTreeSet<usize>,
    ) -> Result<usize> {
        ensure!(
            seen.len() < 1024 && seen.insert(row),
            "Native allocation repeats a schema record"
        );
        p.bytes::<40>(row)?;
        let mut changed = 0;
        if p.u32(row + 16)? == class {
            ensure!(
                p.u32(row + 20)? as usize == counts.0,
                "Native allocation row count differs"
            );
            put(&mut p.0, row + 20, &u32::try_from(counts.1)?.to_le_bytes())?;
            changed += 1;
        }
        if p.u64(row + 8)? != 0 {
            let inherited = p.pointer(row + 8)?;
            ensure!(
                inherited >= 4 && p.u32(inherited - 4)? == 0x80808852,
                "Native inherited allocation schema differs"
            );
            changed += visit(p, inherited, class, counts, seen)?;
        }
        for child in p.array(row + 24, 40, Some(0x80808852))? {
            changed += visit(p, child, class, counts, seen)?;
        }
        Ok(changed)
    }
    visit(p, 8, class, (before, after), &mut BTreeSet::new())
}

/// Write the source named scalar rows into a native receiver, class `80808BF5`. Each runtime
/// row keeps the source's initial scalar, which the native receiver serializes at row `+20`.
pub(super) fn receiver(
    native: &mut Reader,
    target: &mut Payload,
    source: &Payload,
) -> Result<(Value, Option<(u32, Payload)>)> {
    let (si, sd) = (source.pointer(16)?, source.pointer(24)?);
    ensure!(
        source.u32(si - 4)? == 0x808006F7 && source.u32(sd - 4)? == 0x808005A8,
        "Source receiver root differs"
    );
    let source_rows = source.array(sd + 0x90, 0x50, Some(0x80800884))?;
    let source_live = source.array(si + 0x30, 0x28, Some(0x808004E2))?;
    ensure!(
        !source_rows.is_empty()
            && source_rows.len() == source_live.len()
            && source_rows.len() <= 64,
        "Source receiver row count differs"
    );
    let mut rows = Vec::new();
    let mut names = BTreeSet::new();
    for (&row, &live) in source_rows.iter().zip(&source_live) {
        let value = source.f32(live + 0x20)?;
        let name = source.u32(row + 0x30)?;
        ensure!(
            source.u64(row + 8)? == live as u64
                && source.u64(live + 8)? == row as u64
                && (0..4).all(|lane| source.f32(row + 0x40 + lane * 4).ok() == Some(value))
                && value.is_finite()
                && (-100.0..=100.0).contains(&value)
                && names.insert(name),
            "Source receiver row differs from its runtime value or the native clamp"
        );
        rows.push((name, value));
    }
    let (ni, nd) = pair(target)?;
    let owner = target.u32(ni)?;
    ensure!(
        target.u32(ni - 4)? == 0x80808BF5 && target.u32(nd - 4)? == 0x80808BF7,
        "Native receiver root differs"
    );
    let native_rows = target.array(nd + 0x78, 0x40, Some(0x80808BFA))?;
    let native_live = target.array(ni + 0x30, 0x30, Some(0x80808BF9))?;
    let (Some(&row), Some(&live)) = (native_rows.first(), native_live.first()) else {
        anyhow::bail!("Native receiver has no row template");
    };
    ensure!(
        native_rows.len() == native_live.len()
            && target.u64(row + 8)? == live as u64
            && target.u64(live + 8)? == row as u64
            && target.pointer(live + 0x10)? == ni,
        "Native receiver row pair differs"
    );
    // Definition rows name themselves as parent in the checked template.
    ensure!(
        target.pointer(row + 0x10)? == row,
        "Native receiver definition row parent differs"
    );
    let before = native_live.len();
    let definition_row = target.bytes::<0x40>(row)?;
    let live_row = target.bytes::<0x30>(live)?;
    // Each row holds its getter at `+10`, which connections name. With the template's row count
    // the rows are rewritten in place, so those offsets stay inside the live arrays. Otherwise
    // fresh arrays replace them, and getters follow the rows by position.
    if rows.len() != before {
        append(
            &mut target.0,
            ni + 0x30,
            0x80808BF9,
            &live_row.repeat(rows.len()),
            0x30,
        )?;
        append(
            &mut target.0,
            nd + 0x78,
            0x80808BFA,
            &definition_row.repeat(rows.len()),
            0x40,
        )?;
    } else {
        for (&live, &row) in native_live.iter().zip(&native_rows) {
            put(&mut target.0, live, &live_row)?;
            put(&mut target.0, row, &definition_row)?;
        }
    }
    for ((live, row), (name, value)) in target
        .array(ni + 0x30, 0x30, Some(0x80808BF9))?
        .into_iter()
        .zip(target.array(nd + 0x78, 0x40, Some(0x80808BFA))?)
        .zip(&rows)
    {
        put(&mut target.0, live, &owner.to_le_bytes())?;
        put(&mut target.0, live + 4, &0x80808BFAu32.to_le_bytes())?;
        put(&mut target.0, live + 8, &u64::try_from(row)?.to_le_bytes())?;
        relative(target, live + 0x10, ni)?;
        put(&mut target.0, live + 0x20, &value.to_le_bytes())?;
        put(&mut target.0, row, &owner.to_le_bytes())?;
        put(&mut target.0, row + 4, &0x80808BF9u32.to_le_bytes())?;
        put(&mut target.0, row + 8, &u64::try_from(live)?.to_le_bytes())?;
        relative(target, row + 0x10, row)?;
        put(&mut target.0, row + 0x28, &name.to_le_bytes())?;
    }
    let allocation = if rows.len() == before {
        None
    } else {
        let tag = target.u32(0x44)?;
        let mut allocation = (*native.tag(tag, Some(0x80809BBB))?).clone();
        ensure!(
            recount(&mut allocation, 0x80808BF9, before, rows.len())? == 1,
            "Native receiver allocation row is absent or ambiguous"
        );
        Some((tag, allocation))
    };
    finish(target)?;
    Ok((
        json!({"rows":rows.iter().map(|(name, value)| json!({"name":name,"value":value}))
            .collect::<Vec<_>>()}),
        allocation,
    ))
}

fn lookup(p: &Payload, at: usize) -> Result<Vec<(u32, u32, u32)>> {
    p.array(at, 12, None)?
        .into_iter()
        .map(|row| Ok((p.u32(row)?, p.u32(row + 4)?, p.u32(row + 8)?)))
        .collect()
}

/// A channel bank's declaration roles: its declaration count, inherited and incoming ordinals,
/// its public lookup ordinals and the inherited span at `+128`. [`channel`] renames a
/// native bank only to a source bank of the same shape.
#[derive(PartialEq)]
pub(super) struct Shape {
    count: usize,
    inherited: Vec<u32>,
    incoming: Vec<u32>,
    public: Vec<u32>,
    span: [u8; 4],
}

pub(super) fn shape(p: &Payload, old: bool) -> Result<Shape> {
    let root = if old { p.pointer(24)? } else { pair(p)?.1 };
    let ordinals = |at| -> Result<Vec<u32>> {
        let mut found = lookup(p, at)?
            .into_iter()
            .map(|row| row.2)
            .collect::<Vec<_>>();
        found.sort_unstable();
        Ok(found)
    };
    Ok(Shape {
        count: p.array(root + 0xD8, 112, None)?.len(),
        inherited: ordinals(root + 0x108)?,
        incoming: ordinals(root + 0x118)?,
        public: ordinals(root + 0xF8)?,
        span: p.bytes::<4>(root + 0x128)?,
    })
}

/// Rename a native channel bank's declarations to the source's, in source order, when every
/// declaration has the same role. Incoming and inherited lookups take the source rows.
pub(super) fn channel(target: &mut Payload, source: &Payload) -> Result<Value> {
    let s = source.pointer(24)?;
    ensure!(
        source.u32(s - 4)? == 0x808008F6
            && source.u32(source.pointer(16)? - 4)? == 0x80800594
            && source.u64(s + 0xE8)? == 0,
        "Source channel bank root differs"
    );
    let (_, c) = pair(target)?;
    ensure!(
        target.u32(c - 4)? == 0x80809790,
        "Native channel bank root differs"
    );
    let source_rows = source.array(s + 0xD8, 112, Some(0x80800883))?;
    let native_rows = target.array(c + 0xD8, 112, Some(0x808097A1))?;
    let names = source_rows
        .iter()
        .map(|&row| source.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let (inherited, incoming) = (lookup(source, s + 0x108)?, lookup(source, s + 0x118)?);
    let (native_inherited, native_incoming) =
        (lookup(target, c + 0x108)?, lookup(target, c + 0x118)?);
    // Public `+F8` rows export a declaration for other banks on the host to inherit, sorted by
    // name. They carry when both banks export the same ordinals.
    let (public, native_public) = (lookup(source, s + 0xF8)?, lookup(target, c + 0xF8)?);
    let ordinals = |rows: &[(u32, u32, u32)]| {
        let mut found = rows.iter().map(|row| row.2).collect::<Vec<_>>();
        found.sort_unstable();
        found
    };
    ensure!(
        source_rows.len() == native_rows.len()
            && ordinals(&inherited) == ordinals(&native_inherited)
            && ordinals(&incoming) == ordinals(&native_incoming)
            && ordinals(&public) == ordinals(&native_public)
            && source.bytes::<4>(s + 0x128)? == target.bytes::<4>(c + 0x128)?,
        "Native channel bank declarations have other roles"
    );
    for (&from, &to) in source_rows.iter().zip(&native_rows) {
        let source_words = source.array(from + 80, 4, Some(0x8080000B))?;
        let native_words = target.array(to + 80, 4, Some(0x8080000B))?;
        ensure!(
            source.u64(from + 8)? == 0
                && target.u64(to + 8)? == 0
                && source.u64(from + 96)? == 0
                && target.u64(to + 96)? == 0
                && source.bytes::<2>(from + 104)? == [255, 254]
                && target.bytes::<2>(to + 104)? == [255, 254]
                && source_words.len() == native_words.len(),
            "Channel declaration has a procedure, callback or other dependencies"
        );
        for (&a, &b) in source_words.iter().zip(&native_words) {
            let word = source.u32(a)?;
            put(&mut target.0, b, &word.to_le_bytes())?;
        }
    }
    for (&row, &name) in native_rows.iter().zip(&names) {
        put(&mut target.0, row, &name.to_le_bytes())?;
    }
    for (field, rows) in [
        (c + 0x108, &inherited),
        (c + 0x118, &incoming),
        (c + 0xF8, &public),
    ] {
        for (at, &(namespace, name, ordinal)) in
            target.array(field, 12, None)?.into_iter().zip(rows)
        {
            put(&mut target.0, at, &namespace.to_le_bytes())?;
            put(&mut target.0, at + 4, &name.to_le_bytes())?;
            put(&mut target.0, at + 8, &ordinal.to_le_bytes())?;
        }
    }
    // An inherited declaration's slot word at row `+72` is its place in the name-sorted lookup.
    // The copied source lookup orders the same ordinals by the source names, so each inherited
    // row takes its new place. A template slot kept here reads another name's parent channel.
    for (slot, &(_, _, ordinal)) in inherited.iter().enumerate() {
        let row = *native_rows
            .get(usize::try_from(ordinal)?)
            .context("Inherited channel declaration ordinal")?;
        put(&mut target.0, row + 72, &u16::try_from(slot)?.to_le_bytes())?;
    }
    // A declaration's word at row `+6A` is its place in the name-sorted public lookup, `FFFF`
    // when it is not public. The copied source lookup is already name-sorted.
    if !public.is_empty() {
        for (index, &row) in native_rows.iter().enumerate() {
            let slot = public
                .iter()
                .position(|entry| usize::try_from(entry.2).ok() == Some(index))
                .map_or(Ok(0xFFFF), u16::try_from)?;
            put(&mut target.0, row + 0x6A, &slot.to_le_bytes())?;
        }
    }
    Ok(json!({"declarations":names,"inherited":inherited,"incoming":incoming,"public":public}))
}
