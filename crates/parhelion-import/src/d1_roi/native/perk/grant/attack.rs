//! Matched movement routes retain native feedback and privately carry the source fixed amounts.
use super::*;

pub(super) fn lower(
    source: &Source,
    reader: &mut Reader,
    p: &Payload,
    row: usize,
    native: &Payload,
) -> Result<(AbilityRowModifier, Vec<Value>)> {
    ensure!(
        p.u32(p.pointer(16)? - 4)? == 0x808027BE
            && p.u32(p.pointer(24)? - 4)? == 0x808029B5
            && native.u32(native.pointer(16)? - 4)? == 0x80804112
            && native.u32(native.pointer(24)? - 4)? == 0x80804113
            && p.u32(row + 8)? == 255
            && p.u32(row + 16)? == 255,
        "Attack pair requires the matched movement banks and ordinary priority"
    );
    let key = p.u32(row)?;
    let rows = native.array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let rows = rows
        .into_iter()
        .filter(|&r| native.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(
        rows.len() == 1,
        "Native attack property is absent or ambiguous"
    );
    let n = rows[0];
    let sm = p.pointer(row + 32)?;
    let nm = native.pointer(n + 48)?;
    let sg = p.pointer(row + 24)?;
    let ng = native.pointer(n + 40)?;
    ensure!(
        p.u32(sg - 4)? == 0x80802981
            && native.u32(ng - 4)? == 0x8080454A
            && p.u32(sg)? == native.u32(ng)?
            && native.u32(ng + 4)? == 0
            && ![0, u32::MAX, 0x811C9DC5].contains(&p.u32(sg)?)
            && native.u32(n + 24)? == 255
            && native.u32(n + 32)? == 255
            && native.u32(n + 36)? == 0
            && native.u32(nm + 4)? == 0x80804119
            && native.u32(nm - 4)? == 0x8080411A,
        "Native attack property has another gate, mode or modifier"
    );
    let twin = usize::try_from(native.u64(nm + 8)?)?;
    ensure!(
        native.u32(twin + 4)? == 0x8080411A && native.u64(twin + 8)? == nm as u64,
        "Native attack modifier pair is broken"
    );
    let mut evidence = Vec::new();
    let mut amounts = [0; 2];
    for (index, lane) in [0, 8].into_iter().enumerate() {
        ensure!(
            p.u64(sm + lane)? != 0 && native.u64(nm + 16 + lane)? != 0,
            "Attack pair is missing a route"
        );
        let s = p.pointer(sm + lane)?;
        let n = native.pointer(nm + 16 + lane)?;
        ensure!(
            p.u32(s - 4)? == 0x8080240C && native.u32(n - 4)? == 0x80804464,
            "Attack record class differs"
        );
        same(p, s, native, n, 72)?;
        ensure!(
            p.bytes::<8>(s + 88)? == [0; 8]
                && p.u64(s + 8)? == 0
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
        same(p, a + 16, native, b + 44, 52)?;
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
            sa.len() == na.len() && !sa.is_empty() && sa.len() <= 8,
            "Attack trajectory alternatives differ"
        );
        for (s, n) in sa.into_iter().zip(na) {
            let (s, n) = (p.pointer(s)?, native.pointer(n)?);
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
        same(p, a + 0x190, native, b + 0x200, 8)?;
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
        same(p, sd, native, nd, 32)?;
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
        evidence.push(json!({"source_attack":p.u32(a)?,
            "source_damage_profile":p.u32(sd + 40)?, "native_damage_profile":native.u32(nd + 40)?,
            "source_feedback":p.u32(a + 0x150)?, "native_feedback":native.u32(b + 0x188)?,
            "source_profile_lower":source_profile.f32(source_amount)?,
            "source_profile_upper":source_profile.f32(source_amount + 4)?,
            "native_profile_lower":target_profile.f32(target_root + 0x14)?,
            "native_profile_upper":target_profile.f32(target_root + 0x18)?}));
    }
    Ok((
        AbilityRowModifier::Attack {
            template_key: key,
            selection: sundial::package_authoring::ability_bank::FixedSelection {
                name: p.u32(row + 4)?,
                priority: u8::try_from(p.u32(row + 8)?)?,
                required_key: p.u32(sg)?,
            },
            amounts,
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
