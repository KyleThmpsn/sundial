//! Original ability properties must survive in the emitted action and selected bank only.
use super::*;
use sundial::package_authoring::ability_bank::{SLOT_BANKS, property_rows};
mod evade;
mod group;
mod scalar;

pub(super) fn check(
    source: &Source,
    stock: &mut Reader,
    native: &mut Reader,
    action: &Payload,
    choice: &Value,
) -> CheckResult<Value> {
    let grants = choice["ability_grants"].as_array().ok_or("Grant origins")?;
    assert!(!grants.is_empty());
    let effects = action.array(0x38, 8, Some(0x808040AC))?;
    assert_eq!(effects.len(), grants.len());
    let activation = action.array(0x20, 8, Some(0x808040BA))?;
    assert_eq!(activation.len(), 1);
    assert_eq!(action.u8(action.pointer(activation[0])? + 5)?, 0);
    assert!(action.array(0x48, 8, Some(0x808040BA))?.is_empty());
    let mut checked = Vec::new();
    for (grant, effect) in grants.iter().zip(effects) {
        let bank = number(&grant["native_bank"])?;
        let key = number(&grant["property_key"])?;
        let slot = number(&grant["slot"])? as u8;
        let at = action.pointer(effect)?;
        assert_eq!(action.u32(at - 4)?, 0x80803E1D);
        assert_eq!(action.u8(at)?, 7);
        assert_eq!(action.u8(at + 2)?, slot);
        assert_eq!(action.u32(at + 4)?, key);
        assert_eq!(action.u8(at + 8)?, 0);
        let original = source.read(number(&grant["source_bank"])?)?;
        let row = number(&grant["source_property_offset"])? as usize;
        let definition = original.pointer(24)?;
        assert!(
            original
                .array(definition + 0xA0, 40, Some(0x80802A84))?
                .contains(&row)
        );
        assert_eq!(original.u32(row)?, number(&grant["source_key"])?);
        let modifier = original.pointer(row + 32)?;
        if original.u32(modifier - 4)? == 0x808030C8 {
            checked.push(evade::check(stock, native, &original, row, grant)?);
            continue;
        }
        let emitted = native.tag(bank, Some(0x80809C36))?;
        let stored = emitted.array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
        let stored = stored
            .into_iter()
            .filter(|&r| emitted.u32(r + 16).ok() == Some(key))
            .collect::<Vec<_>>();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            emitted.u32(stored[0] + 32)?,
            original.u32(row + 16)?,
            "Property mode changed"
        );
        let before = stock.tag(bank, Some(0x80809C36))?;
        let old = property_rows(&before.0)?;
        let new = property_rows(&emitted.0)?;
        assert!(!old.iter().any(|r| r.key == key));
        // Every stock property remains present with the same handler and values.
        for r in &old {
            assert!(new.iter().any(|n| n.key == r.key
                && n.handler == r.handler
                && n.modifier_class == r.modifier_class
                && n.charge == r.charge
                && n.parameters == r.parameters));
        }
        let rows = new.iter().filter(|r| r.key == key).collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        match original.u32(modifier - 4)? {
            0x8080296E => group::check(&original, row, &before, &emitted, stored[0])?,
            0x8080251A | 0x808028BA | 0x808030A9 | 0x80802591 => {
                let (class, size) = match original.u32(modifier - 4)? {
                    0x8080251A => (0x80804399, 32),
                    0x808028BA => (0x8080442E, 8),
                    0x808030A9 => (0x80804528, 4),
                    _ => (0x8080411F, 4),
                };
                let m = emitted.pointer(stored[0] + 48)?;
                assert_eq!(emitted.u32(m + 4)?, class);
                assert_eq!(
                    &original.0[modifier..modifier + size],
                    &emitted.0[m + 16..m + 16 + size]
                );
                let stock_rows = before.array(before.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
                let anchors = stock_rows
                    .into_iter()
                    .filter(|&r| {
                        before.u32(r + 16).ok() == original.u32(row).ok()
                            && before
                                .pointer(r + 48)
                                .ok()
                                .and_then(|m| before.u32(m + 4).ok())
                                == Some(class)
                    })
                    .collect::<Vec<_>>();
                assert_eq!(anchors.len(), 1);
                assert_eq!(emitted.u32(stored[0] + 28)?, before.u32(anchors[0] + 28)?);
                let sm = before.pointer(anchors[0] + 48)?;
                let retained = emitted
                    .array(emitted.pointer(24)? + 0x88, 56, Some(0x8080453B))?
                    .into_iter()
                    .find(|&r| {
                        emitted.u32(r + 16).ok() == original.u32(row).ok()
                            && emitted
                                .pointer(r + 48)
                                .ok()
                                .and_then(|m| emitted.u32(m + 4).ok())
                                == Some(class)
                    })
                    .ok_or("Stock property missing")?;
                let rm = emitted.pointer(retained + 48)?;
                assert_eq!(
                    &before.0[sm + 16..sm + 16 + size],
                    &emitted.0[rm + 16..rm + 16 + size]
                );
            }
            0x8080235E => scalar::check(&original, row, &before, &emitted, stored[0], grant)?,
            0x80802254 => assert_eq!(rows[0].charge, Some(i64::from(original.u32(modifier)?))),
            0x80802974 => {
                let params = original.array(definition + 0xB0, 16, None)?;
                let first = original.u32(modifier + 16)? as usize;
                let count = original.u32(modifier + 20)? as usize;
                let index = number(&grant["parameter_index"])? as usize;
                assert!((first..first + count).contains(&index));
                let at = params[index];
                assert_eq!(rows[0].parameters.len(), 1);
                let value = &rows[0].parameters[0];
                assert_eq!(value.name, original.u32(at)?);
                assert_eq!(value.reset.to_bits(), original.u32(at + 4)?);
                assert_eq!(value.applied.to_bits(), original.u32(at + 8)?);
                assert_eq!(value.add, original.u32(at + 12)? != 0);
            }
            _ => return Err("Unverified source ability modifier in acceptance".into()),
        }
        for (_, banks) in SLOT_BANKS {
            for &other in banks {
                if other == bank {
                    continue;
                }
                let p = native.tag(other, Some(0x80809C36))?;
                for r in p.array(p.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
                    assert_ne!(
                        p.u32(r + 16)?,
                        key,
                        "Imported property escaped its ability bank"
                    );
                }
            }
        }
        checked.push(json!({"bank":bank,"key":key,"source_row":row}));
    }
    Ok(json!({"ability_grants":checked,"gameplay_verified":false}))
}

/// Follow original grant scope and pattern references, independently of the reported bank.
pub(super) fn origins(
    source: &Source,
    stock: &mut Reader,
    exported: &Value,
    choice: &Value,
) -> CheckResult<()> {
    let talent = &exported["talents"];
    let grid = source.read(number(&talent["shared_grid"])?)?;
    let node_index = number(&choice["node"])? as usize;
    let step_index = number(&choice["step"])? as usize;
    let display = &talent["nodes"][node_index]["steps"][step_index];
    let nodes = grid.array(16, 64, Some(0x80801728))?;
    let steps = grid.array(nodes[node_index] + 24, 144, Some(0x80801758))?;
    let variants = grid.array(
        steps[number(&display["shared_step"])? as usize],
        88,
        Some(0x808016DE),
    )?;
    let variant = variants[display["source_variant"].as_u64().unwrap_or(0) as usize];
    let records = grid.array(variant, 16, Some(0x808018E8))?;
    if records.is_empty() {
        assert!(
            choice["ability_grants"]
                .as_array()
                .ok_or("Grant origins")?
                .is_empty()
        );
        return Ok(());
    }
    let tags = source.classes(0x80803432);
    assert_eq!(tags.len(), 1);
    let definitions = source.read(tags[0])?;
    let abilities = definitions.array(8, 40, Some(0x8080343C))?;
    let mut assignments = BTreeMap::new();
    let assets = source.read(0x80A6A000)?;
    assert_eq!(source.reference(assets.u32(16)?)?, 0x80800341);
    let table = source.read(assets.u32(16)?)?;
    for row in table.array(8, 8, None)? {
        assert!(
            assignments
                .insert(table.u32(row)?, table.u32(row + 4)?)
                .is_none()
        );
    }
    assert!(!assignments.is_empty());
    let target_tags = stock.classes(0x80805CA4);
    assert_eq!(target_tags.len(), 1);
    let targets = stock.tag(target_tags[0], Some(0x80805CA4))?;
    let runtime = stock.tag(0x80EC3F60, Some(0x80809780))?;
    let runtime_rows = runtime.array(8, 8, None)?;
    let mut expected = BTreeSet::new();
    for (index, record) in records.into_iter().enumerate() {
        let key = grid.u32(record)?;
        let selected = grid.u8(record + 4)?;
        assert_eq!(
            grid.bytes::<11>(record + 5)?,
            [0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]
        );
        let mut matched = false;
        for (ordinal, &ability) in abilities.iter().enumerate() {
            if selected != 255 && ordinal != usize::from(selected) {
                continue;
            }
            let pattern = definitions.u32(ability)?;
            let graph = source.read(
                *assignments
                    .get(&pattern)
                    .ok_or("Source ability assignment")?,
            )?;
            for owner in graph.array(32, 12, Some(0x80800715))? {
                let bank = graph.u32(owner)?;
                let payload = source.read(bank)?;
                let settings = payload.pointer(24)?;
                // Component owners without the common property array are not ability banks.
                let Ok(properties) = payload.array(settings + 0xA0, 40, Some(0x80802A84)) else {
                    continue;
                };
                for row in properties {
                    if payload.u32(row)? != key {
                        continue;
                    }
                    matched = true;
                    assert_ne!(
                        key, 0x811C9DC5,
                        "Empty grant unexpectedly names a source property"
                    );
                    assert_eq!(
                        targets
                            .array(8, 28, Some(0x80805CAA))?
                            .into_iter()
                            .filter(|&r| targets.u32(r + 4).ok() == Some(pattern))
                            .count(),
                        1
                    );
                    let matches = runtime_rows
                        .iter()
                        .copied()
                        .filter(|&r| runtime.u32(r).ok() == Some(pattern))
                        .collect::<Vec<_>>();
                    assert_eq!(matches.len(), 1);
                    let native_graph = stock.tag(runtime.u32(matches[0] + 4)?, Some(0x80809C0F))?;
                    let owners = native_graph
                        .array(16, 12, None)?
                        .into_iter()
                        .map(|r| native_graph.u32(r))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let modifier = payload.pointer(row + 32)?;
                    let parameters = match payload.u32(modifier - 4)? {
                        0x80802254 | 0x808030C8 => vec![None],
                        0x80802974 | 0x8080235E => {
                            let first = payload.u32(modifier + 16)?;
                            let count = payload.u32(modifier + 20)?;
                            (first..first + count).map(Some).collect()
                        }
                        _ => {
                            return Err(
                                "Unsupported original property was reported as translated".into()
                            );
                        }
                    };
                    for parameter in parameters {
                        assert!(expected.insert((
                            index as u32,
                            ordinal as u32,
                            bank,
                            row as u32,
                            parameter
                        )));
                    }
                    for reported in choice["ability_grants"]
                        .as_array()
                        .ok_or("Grant report")?
                        .iter()
                        .filter(|r| {
                            r["source_grant"].as_u64() == Some(index as u64)
                                && r["source_ability"].as_u64() == Some(ordinal as u64)
                        })
                    {
                        assert_eq!(number(&reported["source_pattern"])?, pattern);
                        assert!(owners.contains(&number(&reported["native_bank"])?));
                    }
                }
            }
        }
        if !matched {
            assert_eq!(key, 0x811C9DC5, "An unresolved source property was dropped");
            assert_eq!(selected, 255, "An empty property has an ability selection");
        }
    }
    let actual = choice["ability_grants"]
        .as_array()
        .ok_or("Grant report")?
        .iter()
        .map(|r| {
            Ok((
                number(&r["source_grant"])?,
                number(&r["source_ability"])?,
                number(&r["source_bank"])?,
                number(&r["source_property_offset"])?,
                r["parameter_index"]
                    .as_u64()
                    .map(u32::try_from)
                    .transpose()?,
            ))
        })
        .collect::<CheckResult<BTreeSet<_>>>()?;
    assert_eq!(
        actual, expected,
        "Source grant scope or property companions changed"
    );
    assert_eq!(
        actual.len(),
        choice["ability_grants"]
            .as_array()
            .ok_or("Grant report")?
            .len()
    );
    Ok(())
}
