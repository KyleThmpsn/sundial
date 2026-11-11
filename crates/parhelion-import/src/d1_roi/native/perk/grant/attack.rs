//! Matched attack routes retain native feedback and privately carry the source fixed amounts. A
//! movement attack pair (source `80803198`, native `80804119`) names its two records first. A
//! melee attack container (source `808025AC`, native `80804427`) keeps two Float32 ahead of them
//! and appends both records to its attack lists, the source at `010E1354` and native at
//! `0105C7F7`. Its row keeps its source priority, and its gate becomes the key the caller maps.
use super::*;

/// The source gate key of a row, zero when ungated.
fn source_gate(p: &Payload, row: usize) -> Result<u32> {
    if p.u64(row + 24)? == 0 {
        return Ok(0);
    }
    let at = p.pointer(row + 24)?;
    ensure!(
        p.u32(at - 4)? == 0x80802981 && ![0, u32::MAX, 0x811C9DC5].contains(&p.u32(at)?),
        "Source attack gate has another class or key"
    );
    p.u32(at)
}

/// The native gate key of a row, zero when ungated.
fn native_gate(native: &Payload, row: usize) -> Result<u32> {
    if native.u64(row + 40)? == 0 {
        return Ok(0);
    }
    let at = native.pointer(row + 40)?;
    ensure!(
        native.u32(at - 4)? == 0x8080454A && native.u32(at + 4)? == 0,
        "Native attack gate has another class"
    );
    native.u32(at)
}

pub(super) fn lower(
    source: &Source,
    reader: &mut Reader,
    p: &Payload,
    row: usize,
    native: &Payload,
    gate: Option<u32>,
) -> Result<(AbilityRowModifier, Vec<Value>)> {
    let markers = |data: &Payload| -> Result<(u32, u32)> {
        Ok((
            data.u32(data.pointer(16)? - 4)?,
            data.u32(data.pointer(24)? - 4)?,
        ))
    };
    let sm = p.pointer(row + 32)?;
    let melee = p.u32(sm - 4)? == 0x808025AC;
    let (source_banks, native_banks, class) = if melee {
        (
            (0x80802883, 0x80802200),
            (0x808041B6, 0x808041B7),
            0x80804427,
        )
    } else {
        (
            (0x808027BE, 0x808029B5),
            (0x80804112, 0x80804113),
            0x80804119,
        )
    };
    ensure!(
        markers(p)? == source_banks
            && markers(native)? == native_banks
            && (melee || p.u32(row + 8)? == 255)
            && p.u32(row + 8)? <= 255
            && p.u32(row + 16)? == 255,
        "Attack requires matched movement or melee banks and a qualified priority"
    );
    let key = p.u32(row)?;
    let rows = native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let rows = rows
        .into_iter()
        .filter(|&r| {
            native.u32(r + 16).ok() == Some(key)
                && native.pointer(r + 48).and_then(|m| native.u32(m + 4)).ok() == Some(class)
        })
        .collect::<Vec<_>>();
    ensure!(
        rows.len() == 1,
        "Native attack property is absent or ambiguous"
    );
    let n = rows[0];
    let nm = native.pointer(n + 48)?;
    let required = source_gate(p, row)?;
    // A movement attack is restricted to its subclass's key. A melee container's gate, when it
    // has one, is its base ability's key, which the caller maps.
    ensure!(
        required == native_gate(native, n)?
            && (if melee {
                (required == 0) == gate.is_none()
            } else {
                required != 0
            })
            && native.u32(n + 24)? == p.u32(row + 8)?
            && native.u32(n + 32)? == 255
            && native.u32(n + 36)? == 0
            && native.u32(nm - 4)? == class + 1,
        "Native attack property has another gate, priority, mode or modifier"
    );
    let twin = usize::try_from(native.u64(nm + 8)?)?;
    ensure!(
        native.u32(twin + 4)? == class + 1 && native.u64(twin + 8)? == nm as u64,
        "Native attack modifier pair is broken"
    );
    // Where each body names its two records.
    let (source_records, native_records) = if melee {
        ensure!(
            p.bytes::<8>(sm)? == native.bytes::<8>(nm + 0x10)?
                && p.u64(sm + 0x18)? == 0
                && p.u32(sm + 0x20)? == 0
                && native.u64(nm + 0x28)? == 0
                && native.u32(nm + 0x30)? == 0,
            "Melee attack container has other leading values or a third route"
        );
        (sm + 8, nm + 0x18)
    } else {
        (sm, nm + 16)
    };
    let mut evidence = Vec::new();
    let mut amounts = [0; 2];
    // Each melee record's distance controls, source and native, in `ability_bank::CONTROLS`
    // order: profile reach and range, lunge range and next lane, contact control.
    let mut controls = [[0; 5]; 2];
    let mut native_controls = [[0; 5]; 2];
    for (index, lane) in [0, 8].into_iter().enumerate() {
        ensure!(
            p.u64(source_records + lane)? != 0 && native.u64(native_records + lane)? != 0,
            "Attack pair is missing a route"
        );
        let s = p.pointer(source_records + lane)?;
        let n = native.pointer(native_records + lane)?;
        ensure!(
            p.u32(s - 4)? == 0x8080240C && native.u32(n - 4)? == 0x80804464,
            "Attack record class differs"
        );
        same(p, s, native, n, 72)?;
        // Source melee records hold a word of 1 at +58 ahead of two zero words, where movement
        // records hold zero. Its consumer is not read. The native record, which keeps its own
        // profile selector there, is copied with the fields compared here. A source record ends
        // at +64, where the next block's marker can follow.
        let selection_word = p.u32(s + 88)?;
        ensure!(
            (if melee {
                selection_word <= 1 && p.u32(s + 92)? == 0 && p.u32(s + 96)? == 0
            } else {
                p.bytes::<8>(s + 88)? == [0; 8]
            }) && p.u64(s + 8)? == 0
                && p.u64(s + 16)? == 0
                && p.u32(s + 32)? == u32::MAX
                && p.u32(s + 56)? == u32::MAX,
            "Source attack has additional metadata"
        );
        let a = p.array(s + 72, 408, Some(0x80802A15))?;
        let b = native.array(n + 72, 544, Some(0x80802D7B))?;
        ensure!(
            a.len() == 1 && b.len() == 1,
            "Attack pair requires one profile per route"
        );
        let (a, b) = (a[0], b[0]);
        let selections = native.array(n + 88, 8, Some(0x80802EAE))?;
        ensure!(
            selections.len() == 1 && native.pointer(selections[0])? == b,
            "Native attack selection reaches another profile"
        );
        ensure!(
            native.u32(b)? == native.u32(b + 4)?,
            "Native attack identity lanes differ"
        );
        same(p, a, native, b + 4, 12)?;
        ensure!(p.u32(a + 12)? == 0, "Source attack has additional flags");
        // The control block's first Float32 is the attack's reach. Shadowkeep shortened the
        // Warlock melee containers' distances (Scorch, Brimstone and Solar Wind store 6 in every
        // source distance lane, and 4.5 or 5.5 natively), so a melee row keeps its five source
        // distance lanes. Every other control lane must agree.
        if melee {
            controls[index][0] = p.u32(a + 16)?;
            native_controls[index][0] = native.u32(b + 44)?;
            same(p, a + 20, native, b + 48, 48)?;
        } else {
            same(p, a + 16, native, b + 44, 52)?;
        }
        for (from, to) in [(0x48, 0x60), (0x80, 0x98), (0xB8, 0xD0)] {
            literal(p, a + from, native, b + to)?;
        }
        same(p, a + 0xF0, native, b + 0x108, 16)?;
        ensure!(
            p.bytes::<8>(a + 0x100)? == [0; 8] && p.bytes::<16>(a + 0x118)? == [0; 16],
            "Source attack has an additional trajectory or input"
        );
        let sa = p.array(a + 0x108, 8, Some(0x808028AF))?;
        let na = native.array(b + 0x118, 8, Some(0x80804446))?;
        ensure!(
            sa.len() == na.len() && !sa.is_empty() && sa.len() <= 8 && (!melee || sa.len() == 1),
            "Attack trajectory alternatives differ"
        );
        for (s, n) in sa.into_iter().zip(na) {
            let (s, n) = (p.pointer(s)?, native.pointer(n)?);
            if melee {
                // A melee lunge (source `80802A9C`, native `80804445`) keeps its first word and
                // its speed pair, and maps its range and next lane as the movement trajectory
                // does (`+4` to `+20`, `+8` to `+30`).
                ensure!(
                    p.u32(s - 4)? == 0x80802A9C && native.u32(n - 4)? == 0x80804445,
                    "Melee lunge requires another translation"
                );
                same(p, s, native, n, 4)?;
                same(p, s + 0xC, native, n + 0x34, 8)?;
                controls[index][2] = p.u32(s + 4)?;
                controls[index][3] = p.u32(s + 8)?;
                native_controls[index][2] = native.u32(n + 0x20)?;
                native_controls[index][3] = native.u32(n + 0x30)?;
                continue;
            }
            ensure!(
                p.u32(s - 4)? == 0x80802975
                    && native.u32(n - 4)? == 0x80804444
                    && p.u32(s + 12)? == 0,
                "Attack trajectory requires another translation"
            );
            for (from, to) in [
                (0, 0),
                (4, 32),
                (8, 48),
                (16, 60),
                (20, 36),
                (24, 40),
                (20, 68),
                (24, 72),
            ] {
                same(p, s + from, native, n + to, 4)?;
            }
        }
        same(p, a + 0x128, native, b + 0x150, 24)?;
        same(p, a + 0x140, native, b + 0x174, 8)?;
        same(p, a + 0x168, native, b + 0x1A0, 8)?;
        if melee {
            same(p, a + 0x190, native, b + 0x200, 4)?;
            controls[index][1] = p.u32(a + 0x194)?;
            native_controls[index][1] = native.u32(b + 0x204)?;
        } else {
            same(p, a + 0x190, native, b + 0x200, 8)?;
        }
        ensure!(
            p.bytes::<24>(a + 0x178)? == [0; 24]
                && p.u32(a + 0x160)? == u32::MAX
                && p.u32(a + 0x164)? == 0,
            "Source attack has additional behavior"
        );
        ensure!(
            source.reference(p.u32(a + 0x150)?)? == 0x80800734,
            "Source attack feedback is not an entity graph"
        );
        reader.tag(native.u32(b + 0x188)?, Some(0x80809C0F))?;
        let (sd, nd) = (p.pointer(a + 0x170)?, native.pointer(b + 0x1A8)?);
        ensure!(
            p.u32(sd - 4)? == 0x808031A4 && native.u32(nd - 4)? == 0x80804455,
            "Attack contact controls have another class"
        );
        if melee {
            same(p, sd, native, nd, 8)?;
            same(p, sd + 12, native, nd + 12, 20)?;
            controls[index][4] = p.u32(sd + 8)?;
            native_controls[index][4] = native.u32(nd + 8)?;
            ensure!(
                controls[index]
                    .iter()
                    .all(|&value| f32::from_bits(value).is_finite() && f32::from_bits(value) >= 0.0),
                "Source attack distance controls are not numbers"
            );
        } else {
            same(p, sd, native, nd, 32)?;
        }
        same(p, sd + 64, native, nd + 64, 16)?;
        let source_profile = source.read(p.u32(sd + 40)?)?;
        let source_root = source_profile.pointer(24)?;
        let declaration = source.read(source_profile.u32(source_root - 4)?)?;
        ensure!(
            declaration.u32(16)? == 0x80802364,
            "Source attack profile has another base"
        );
        let source_amount = source_profile.pointer(source_root + 24)?;
        let target_profile = reader.tag(native.u32(nd + 40)?, Some(0x80809C36))?;
        let target_root = target_profile.pointer(24)?;
        ensure!(
            target_profile.u32(target_root - 4)? == 0x80804B02,
            "Native attack profile has another base"
        );
        ensure!(
            source_profile.u8(source_amount + 12)? == target_profile.u8(target_root + 32)?,
            "Source and native attack damage types differ"
        );
        let amount = source_profile.f32(source_amount)?;
        ensure!(
            amount.is_finite()
                && amount > 0.0
                && source_profile.u32(source_amount)? == source_profile.u32(source_amount + 4)?
                && source_profile.u32(source_amount + 8)? == 0
                && target_profile.u8(target_root + 16)? == 0
                && target_profile.u8(target_root + 0x53)? == 0,
            "Attack amount requires a fixed source and an absolute native profile without clock scaling"
        );
        amounts[index] = amount.to_bits();
        reader.tag(native.u32(nd + 56)?, None)?;
        evidence.push(
            json!({"source_attack":p.u32(a)?,"source_record_word_58":selection_word,
            "source_controls":controls[index].map(f32::from_bits),
            "native_controls":native_controls[index].map(f32::from_bits),
            "source_damage_profile":p.u32(sd + 40)?, "native_damage_profile":native.u32(nd + 40)?,
            "source_feedback":p.u32(a + 0x150)?, "native_feedback":native.u32(b + 0x188)?,
            "source_profile_lower":source_profile.f32(source_amount)?,
            "source_profile_upper":source_profile.f32(source_amount + 4)?,
            "native_profile_lower":target_profile.f32(target_root + 0x14)?,
            "native_profile_upper":target_profile.f32(target_root + 0x18)?}),
        );
    }
    Ok((
        AbilityRowModifier::Attack {
            template_key: key,
            selection: sundial::package_authoring::ability_bank::FixedSelection {
                name: p.u32(row + 4)?,
                priority: u8::try_from(p.u32(row + 8)?)?,
                required_key: if melee { gate.unwrap_or(0) } else { required },
            },
            amounts,
            controls: (melee && controls != native_controls).then_some(controls),
        },
        evidence,
    ))
}

fn same(a: &Payload, x: usize, b: &Payload, y: usize, len: usize) -> Result<()> {
    ensure!(
        a.0.get(x..x + len).context("Source attack extent")?
            == b.0.get(y..y + len).context("Native attack extent")?,
        "Attack controls differ at source {x:X}, native {y:X}"
    );
    Ok(())
}

fn literal(a: &Payload, x: usize, b: &Payload, y: usize) -> Result<()> {
    same(a, x, b, y, 16)?;
    same(a, x + 48, b, y + 48, 8)?;
    ensure!(
        a.u64(x + 16)? == 4 && a.u64(x + 32)? == 1 && a.u32(x + 48)? == 1,
        "Attack scalar is not a single constant"
    );
    let code = a.array(x + 16, 1, Some(0x80800009))?;
    ensure!(
        a.bytes::<4>(code[0])? == [0x34, 0, 0x3E, 0],
        "Attack scalar requires another program"
    );
    for (offset, stride, class) in [(16, 1, 0x80800009), (32, 16, 0x80800090)] {
        let left = a.array(x + offset, stride, Some(class))?;
        let right = b.array(y + offset, stride, Some(class))?;
        ensure!(
            left.len() == right.len() && !left.is_empty() && left.len() <= 64,
            "Attack scalar program differs"
        );
        for (l, r) in left.into_iter().zip(right) {
            same(a, l, b, r, stride)?;
        }
    }
    ensure!(a.u64(x + 8)? == 0, "Attack scalar requires a dynamic input");
    Ok(())
}
