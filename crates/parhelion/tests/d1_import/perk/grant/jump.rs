//! Complete source groups are reached through one emitted action, with count-only gate expansion.
use super::*;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    native: &mut Reader,
    grant: &Value,
) -> CheckResult<Value> {
    let source_key = number(&grant["source_key"])?;
    let key = number(&grant["property_key"])?;
    let bank = number(&grant["native_bank"])?;
    let original = source.read(number(&grant["source_bank"])?)?;
    let source_rows = original
        .array(original.pointer(24)? + 0xA0, 40, Some(0x80802A84))?
        .into_iter()
        .filter(|&row| original.u32(row).ok() == Some(source_key))
        .collect::<Vec<_>>();
    assert!(!source_rows.is_empty());
    let before = stock.tag(bank, Some(0x80809C36))?;
    let emitted = native.tag(bank, Some(0x80809C36))?;
    let old_rows = before.array(before.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    assert!(
        !old_rows
            .iter()
            .any(|&row| before.u32(row + 16).ok() == Some(key))
    );
    let rows = emitted
        .array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?
        .into_iter()
        .filter(|&row| emitted.u32(row + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    let mut cursor = 0;
    let mut checked = Vec::new();
    for source_row in source_rows {
        let source_body = original.pointer(source_row + 32)?;
        let (class, size) = match original.u32(source_body - 4)? {
            0x808027E0 => (0x808044D2, 1),
            0x80802330 => (0x808044D4, 8),
            0x80802BC2 => (0x808044D6, 16),
            0x8080209C => (0x808042A5, 4),
            _ => return Err("Unqualified source Jump body in emitted group".into()),
        };
        let source_gate = if original.u64(source_row + 24)? == 0 {
            None
        } else {
            let gate = original.pointer(source_row + 24)?;
            assert_eq!(original.u32(gate - 4)?, 0x80802981);
            assert_eq!(class, 0x808042A5, "Non-idempotent gate was expanded");
            Some(original.u32(gate)?)
        };
        let count = if source_gate.is_some() { 2 } else { 1 };
        let mut gates = Vec::new();
        for row in rows
            .get(cursor..cursor + count)
            .ok_or("Incomplete Jump group")?
        {
            let body = emitted.pointer(row + 48)?;
            assert_eq!(emitted.u32(body + 4)?, class);
            assert_eq!(
                &emitted.0[body + 16..body + 16 + size],
                &original.0[source_body..source_body + size]
            );
            assert_eq!(emitted.u32(row + 20)?, original.u32(source_row + 4)?);
            assert_eq!(emitted.u32(row + 24)?, original.u32(source_row + 8)?);
            assert_eq!(emitted.u32(row + 32)?, original.u32(source_row + 16)?);
            let gate = if emitted.u64(row + 40)? == 0 {
                None
            } else {
                let at = emitted.pointer(row + 40)?;
                assert_eq!(emitted.u32(at - 4)?, 0x8080454A);
                assert_eq!(emitted.u32(at + 4)?, 0);
                Some(emitted.u32(at)?)
            };
            gates.push(gate);
            let instance = usize::try_from(emitted.u64(row + 8)?)?;
            let twin = emitted.pointer(instance + 48)?;
            assert_eq!(emitted.u64(instance + 8)?, *row as u64);
            assert_eq!(emitted.u32(twin + 4)?, class + 1);
            assert_eq!(emitted.u64(twin + 8)?, body as u64);
            assert_eq!(emitted.u64(body + 8)?, twin as u64);
            checked.push(json!({"row":row,"class":class,"gate":gate,
                "body":hex::encode(&emitted.0[body + 16..body + 16 + size]),
                "priority":emitted.u32(row + 24)?,"handler":emitted.u32(row + 28)?}));
        }
        match source_gate {
            None => assert_eq!(gates, [None]),
            Some(stock_key) => {
                assert_eq!(gates[0], Some(stock_key));
                let private = gates[1].ok_or("Private source membership gate")?;
                assert!(![0, u32::MAX, 0x811C9DC5, stock_key, key].contains(&private));
                assert!(
                    !old_rows
                        .iter()
                        .any(|&row| before.u32(row + 16).ok() == Some(private))
                );
                // The combined ability E2E independently follows the movement publisher and
                // checks that this second operand is its actual emitted activation key.
            }
        }
        cursor += count;
    }
    assert_eq!(
        cursor,
        rows.len(),
        "Emitted Jump group gained an unrelated companion"
    );
    for (_, banks) in SLOT_BANKS {
        for &other in banks {
            if other == bank {
                continue;
            }
            let data = native.tag(other, Some(0x80809C36))?;
            for row in data.array(data.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
                assert_ne!(data.u32(row + 16)?, key, "Jump group escaped its bank");
            }
        }
    }
    Ok(json!({"bank":bank,"key":key,"source_key":source_key,"rows":checked}))
}
