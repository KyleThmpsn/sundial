//! Private attack routes retain their gate and feedback while carrying the source amounts.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::Graph;

pub(super) fn check(
    packages: &Source,
    stock: &mut Reader,
    native: &mut Reader,
    source: &Payload,
    source_row: usize,
    grant: &Value,
) -> CheckResult<Value> {
    assert_eq!(grant["route"], "private_attack_pair");
    assert_eq!(grant["balance"], "source_amounts_native_feedback");
    let bank = number(&grant["native_bank"])?;
    let key = number(&grant["property_key"])?;
    let source_key = source.u32(source_row)?;
    assert_ne!(key, source_key);
    let before = stock.tag(bank, Some(0x80809C36))?;
    let after = native.tag(bank, Some(0x80809C36))?;
    let find = |p: &Payload, key: u32| -> CheckResult<usize> {
        let rows = p.array(p.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
        let rows = rows
            .into_iter()
            .filter(|&r| p.u32(r + 16).ok() == Some(key))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        Ok(rows[0])
    };
    let (old, new) = (find(&before, source_key)?, find(&after, key)?);
    let retained = find(&after, source_key)?;
    assert_eq!(
        before.bytes::<24>(old + 16)?,
        after.bytes::<24>(retained + 16)?
    );
    assert_eq!(after.u32(new + 20)?, source.u32(source_row + 4)?);
    assert_eq!(after.u32(new + 24)?, source.u32(source_row + 8)?);
    assert_eq!(after.u32(new + 28)?, before.u32(old + 28)?);
    assert_eq!(after.u32(new + 32)?, source.u32(source_row + 16)?);
    let gate = after.pointer(new + 40)?;
    assert_eq!(after.u32(gate - 4)?, 0x8080454A);
    assert_eq!(
        after.u32(gate)?,
        source.u32(source.pointer(source_row + 24)?)?
    );
    let sm = source.pointer(source_row + 32)?;
    let om = before.pointer(old + 48)?;
    let nm = after.pointer(new + 48)?;
    let rm = after.pointer(retained + 48)?;
    assert_eq!(after.u32(nm + 4)?, 0x80804119);
    let mut attacks = Vec::new();
    for lane in [0, 8] {
        let s = source.pointer(sm + lane)?;
        let o = before.pointer(om + 16 + lane)?;
        let n = after.pointer(nm + 16 + lane)?;
        assert_eq!(source.u32(s - 4)?, 0x8080240C);
        assert_eq!(after.u32(n - 4)?, 0x80804464);
        assert_eq!(before.bytes::<72>(o)?, after.bytes::<72>(n)?);
        let retained_root = after.pointer(rm + 16 + lane)?;
        let sp = source.array(s + 72, 408, Some(0x80802A15))?;
        let op = before.array(o + 72, 544, Some(0x80802D7B))?;
        let np = after.array(n + 72, 544, Some(0x80802D7B))?;
        assert_eq!((sp.len(), op.len(), np.len()), (1, 1, 1));
        // The secondary selector points into the profile array, without a separate
        // marker. Follow that declared interior reference before capturing the profile.
        let selected = after.array(n + 88, 8, Some(0x80802EAE))?;
        assert_eq!(selected.len(), 1);
        assert_eq!(after.pointer(selected[0])?, np[0]);
        let retained_profile = after.array(retained_root + 72, 544, Some(0x80802D7B))?;
        assert_eq!(retained_profile.len(), 1);
        let stock_graph = Graph::read(&before.0, op[0], 0x80802D7B)?;
        assert_eq!(
            stock_graph,
            Graph::read(&after.0, retained_profile[0], 0x80802D7B)?
        );
        assert_eq!(source.u32(sp[0])?, after.u32(np[0])?);
        assert_eq!(
            source.bytes::<52>(sp[0] + 16)?,
            after.bytes::<52>(np[0] + 44)?
        );
        let sd = source.pointer(sp[0] + 368)?;
        let nd = after.pointer(np[0] + 424)?;
        assert_eq!(source.u32(sd - 4)?, 0x808031A4);
        assert_eq!(after.u32(nd - 4)?, 0x80804455);
        assert_eq!(source.bytes::<32>(sd)?, after.bytes::<32>(nd)?);
        assert_eq!(source.bytes::<16>(sd + 64)?, after.bytes::<16>(nd + 64)?);
        let original_damage = before.u32(before.pointer(op[0] + 424)? + 40)?;
        let private_damage = after.u32(nd + 40)?;
        assert_ne!(private_damage, original_damage);
        let original_profile = stock.tag(original_damage, Some(0x80809C36))?;
        assert_eq!(
            original_profile.0,
            native.tag(original_damage, Some(0x80809C36))?.0
        );
        let source_profile = packages.read(source.u32(sd + 40)?)?;
        let amount = source_profile.pointer(source_profile.pointer(24)? + 24)?;
        let private_profile = native.tag(private_damage, Some(0x80809C36))?;
        let root = private_profile.pointer(24)?;
        assert_eq!(private_profile.u8(root + 16)?, 0);
        assert_eq!(
            private_profile.bytes::<12>(root + 20)?,
            source_profile.bytes::<12>(amount)?
        );
        assert_eq!(
            private_profile.u8(root + 32)?,
            source_profile.u8(amount + 12)?
        );
        let mut expected_profile = original_profile.0.clone();
        let old_root = original_profile.pointer(24)?;
        assert_eq!(root, old_root);
        for at in [old_root, original_profile.u64(old_root + 8)? as usize] {
            expected_profile[at..at + 4].copy_from_slice(&private_damage.to_le_bytes());
        }
        expected_profile[old_root + 20..old_root + 32]
            .copy_from_slice(&source_profile.0[amount..amount + 12]);
        assert_eq!(private_profile.0, expected_profile);
        let mut expected_graph = stock_graph;
        let mut contacts = 0;
        for block in &mut expected_graph.blocks {
            if block.class == 0x80804455 {
                assert_eq!(
                    u32::from_le_bytes(block.bytes[40..44].try_into()?),
                    original_damage
                );
                block.bytes[40..44].copy_from_slice(&private_damage.to_le_bytes());
                contacts += 1;
            }
        }
        assert_eq!(contacts, 1);
        assert_eq!(expected_graph, Graph::read(&after.0, np[0], 0x80802D7B)?);
        attacks.push(json!({"source_name":source.u32(sp[0])?,
            "native_damage_profile":after.u32(nd + 40)?,"native_feedback":after.u32(nd + 56)?}));
    }
    Ok(
        json!({"bank":bank,"key":key,"attacks":attacks,"balance":"source_amounts_native_feedback","gameplay_verified":false}),
    )
}
