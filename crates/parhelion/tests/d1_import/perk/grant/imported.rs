//! A grant to a source Super with no Shadowkeep version applies the source key on the Super slot,
//! where only an imported copy of that Super answers it.
//!
//! The route is wrong when the applied key is not the source key, when the slot is not the Super
//! slot, when the source bank has no row under the key, when a row under it selects an effect
//! graph the imported copy may lack, or when a stock Super bank answers the key as a row or a
//! gate. That an imported copy keeps the key's rows is the subclass preparation's readback.
use super::*;
use sundial::package_authoring::ability_bank::{AbilityTarget, membership_keys};

pub(super) fn check(source: &Source, stock: &mut Reader, grant: &Value) -> CheckResult<Value> {
    let key = number(&grant["property_key"])?;
    assert_eq!(key, number(&grant["source_key"])?);
    assert_eq!(
        number(&grant["slot"])?,
        u32::from(AbilityTarget::Super.byte())
    );
    let original = source.read(number(&grant["source_bank"])?)?;
    let definition = original.pointer(24)?;
    let rows = original
        .array(definition + 0xA0, 40, Some(0x80802A84))?
        .into_iter()
        .filter(|&row| original.u32(row).ok() == Some(key))
        .collect::<Vec<_>>();
    assert!(!rows.is_empty(), "The source bank has no row under the key");
    for &row in &rows {
        let modifier = original.pointer(row + 32)?;
        assert!(
            !matches!(original.u32(modifier - 4)?, 0x8080216B | 0x8080236B),
            "A row under the key selects an effect graph"
        );
        assert_eq!(original.u64(row + 24)?, 0, "A row under the key is gated");
    }
    let banks = SLOT_BANKS
        .iter()
        .find(|(slot, _)| *slot == AbilityTarget::Super)
        .ok_or("Super slot banks")?
        .1;
    assert!(!banks.is_empty());
    for &bank in banks {
        let payload = stock.tag(bank, Some(0x80809C36))?;
        assert!(
            !property_rows(&payload.0)?.iter().any(|row| row.key == key)
                && !membership_keys(&payload.0)?.contains(&key),
            "Stock Super bank {bank:08X} answers the imported key"
        );
    }
    Ok(json!({"imported_ability_key":key,"source_rows":rows.len(),
        "stock_super_banks":banks.len(),"gameplay_verified":false}))
}
