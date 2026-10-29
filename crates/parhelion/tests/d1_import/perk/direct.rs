//! Follow an emitted direct-effect operand to its private outgoing profile.
use super::*;

pub(super) fn evidence(reader: &mut Reader, action: &Payload) -> CheckResult<Vec<Value>> {
    let mut profiles = Vec::new();
    for row in action.array(0x38, 8, Some(0x808040AC))? {
        let effect = action.pointer(row)?;
        if action.u8(effect)? != 4 {
            continue;
        }
        let tag = action.u32(effect + 16)?;
        let owner = reader.tag(tag, Some(0x80809C36))?;
        let root = owner.pointer(24)?;
        let responses = if owner.u64(root + 0x78)? == 0 {
            Vec::new()
        } else {
            owner
                .array(root + 0x78, 8, Some(0x80804B3D))?
                .into_iter()
                .map(|row| {
                    let at = owner.pointer(row)?;
                    Ok(json!({"class": owner.u32(at - 4)?,
                        "flags": owner.bytes::<4>(at)?,
                        "amount_bits": owner.bytes::<8>(at + 4)?}))
                })
                .collect::<CheckResult<Vec<_>>>()?
        };
        profiles.push(json!({"profile": tag, "target": action.u8(effect + 2)?,
            "amount_bits": owner.u32(root + 0x14)?, "damage_type": owner.u8(root + 0x20)?,
            "responses": responses,
            "allocation": owner.u32(0x44)?, "bytes": owner.0.len()}));
    }
    Ok(profiles)
}

pub(super) fn check(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    s: usize,
    action: &Payload,
    n: usize,
) -> CheckResult<()> {
    assert_eq!(source.u32(s - 4)?, 0x808025A9);
    assert_eq!(action.u32(n - 4)?, 0x80803E46);
    assert_eq!(action.u8(n)?, 4);
    assert_eq!(source.bytes::<7>(s + 1)?, action.bytes::<7>(n + 1)?);
    assert_eq!(action.u64(n + 8)?, 0);
    let original = reader.read(source.u32(s + 16)?)?;
    let definition = original.pointer(24)?;
    let amount = original.pointer(definition + 24)?;
    let private = action.u32(n + 16)?;
    assert!(
        stock
            .manager
            .get_entry(tiger_pkg::TagHash(private))
            .is_none()
    );
    let owner = emitted.tag(private, Some(0x80809C36))?;
    let (runtime, root) = (owner.pointer(16)?, owner.pointer(24)?);
    assert_eq!(owner.u32(runtime - 4)?, 0x80804B01);
    assert_eq!(owner.u32(root - 4)?, 0x80804B02);
    assert_eq!(owner.u32(runtime)?, private);
    assert_eq!(owner.u32(root)?, private);
    assert_eq!(owner.u64(runtime + 8)?, root as u64);
    assert_eq!(owner.u64(root + 8)?, runtime as u64);
    assert_eq!(
        owner.bytes::<12>(root + 0x14)?,
        original.bytes::<12>(amount)?
    );
    assert_eq!(owner.u8(root + 0x20)?, original.u8(amount + 12)?);
    assert_eq!(owner.u8(root + 0x10)?, 0, "Amount must remain absolute");
    let healing = original.u64(definition + 0x28)? != 0;
    if healing {
        let source_rows = original.array(definition + 0x28, 8, Some(0x8080276E))?;
        let native_rows = owner.array(root + 0x78, 8, Some(0x80804B3D))?;
        assert_eq!(source_rows.len(), 1);
        assert_eq!(native_rows.len(), 1);
        let source_response = original.pointer(source_rows[0])?;
        let native_response = owner.pointer(native_rows[0])?;
        assert_eq!(original.u32(source_response - 4)?, 0x80802976);
        assert_eq!(owner.u32(native_response - 4)?, 0x80804B5D);
        assert_eq!(
            original.bytes::<12>(source_response)?,
            owner.bytes::<12>(native_response)?
        );
        assert_eq!(owner.u32(native_response + 12)?, 0x811C9DC5);
        assert_eq!(
            owner.u32(root + 0xA0)?,
            original.u32(original.pointer(definition + 0x38)?)?
        );
        let source_labels = original.array(definition + 0xC8, 24, Some(0x80800365))?;
        let native_labels = owner.array(root + 0x60, 24, Some(0x808094B3))?;
        assert_eq!(source_labels.len(), native_labels.len());
        let mut source_mask = [0u8; 32];
        let mut native_mask = [0u8; 40];
        for (&s, &n) in source_labels.iter().zip(&native_labels) {
            let name = original.u32(s)?;
            assert_eq!(owner.u32(n)?, name);
            let dictionary = reader.read(original.u32(s + 16)?)?;
            let atoms = dictionary.array(8, 4, Some(0x80800070))?;
            let index = atoms
                .iter()
                .position(|&r| dictionary.u32(r).ok() == Some(name))
                .ok_or("Missing source health category")?;
            source_mask[index / 8] |= 1 << (index % 8);
            let dictionary = emitted.tag(owner.u32(n + 16)?, None)?;
            let atoms = dictionary.array(8, 4, Some(0x80800070))?;
            let index = atoms
                .iter()
                .position(|&r| dictionary.u32(r).ok() == Some(name))
                .ok_or("Missing native health category")?;
            native_mask[index / 8] |= 1 << (index % 8);
        }
        assert_eq!(original.bytes::<32>(amount + 0x10)?, source_mask);
        assert_eq!(owner.bytes::<40>(root + 0x24)?, native_mask);
    } else {
        assert_eq!(owner.bytes::<40>(root + 0x24)?, [0; 40]);
        assert_eq!(original.bytes::<32>(amount + 0x10)?, [0; 32]);
        for offset in [0x60, 0x78] {
            assert_eq!(owner.bytes::<16>(root + offset)?, [0; 16]);
        }
    }
    assert_eq!(owner.u8(root + 0x53)?, 0, "No clock scaling");
    assert_eq!(owner.u64(root + 0x58)?, 0, "No donor predicate");
    for offset in [0x90, 0xB8, 0xF0] {
        assert_eq!(owner.bytes::<16>(root + offset)?, [0; 16]);
    }
    assert_eq!(owner.u32(root + 0xB0)?, u32::MAX, "No donor feedback");
    let allocation = emitted.tag(owner.u32(0x44)?, Some(0x80809BBB))?;
    assert_eq!(allocation.0.len(), 48);
    assert_eq!(allocation.u64(16)?, 0);
    assert_eq!(allocation.bytes::<16>(32)?, [0; 16]);
    // Read back the actual retained carrier too. Private amounts must never replace stock data.
    let template = if healing { 0x8162C86E } else { 0x815B2062 };
    assert_eq!(
        stock.tag(template, Some(0x80809C36))?.0,
        emitted.tag(template, Some(0x80809C36))?.0
    );
    Ok(())
}
