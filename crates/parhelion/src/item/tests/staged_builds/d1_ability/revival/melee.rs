//! Source melee rows. A placed melee carries its foundation's rows and its upgrade's as private
//! rows of the native melee bank, each source class under its native counterpart: charges, the
//! flag word with its bits one place lower, the HUD-hiding byte, the source HUD name (a native
//! glyph row), the contact effect keeping the native entity, attack containers with private
//! source-amount profiles, and numeric rows with their source factor. Upgrade rows keep their
//! source priority and gate on a private key of the foundation, never on the stock key. A row of
//! another class, value, priority or gate, a stock damage profile or a missing glyph is a failure.
use super::*;

const ROW_KEY: usize = 0x10;
const ROW_PRIORITY: usize = 0x18;
const ROW_GATE: usize = 0x28;
const ROW_BODY: usize = 0x30;

fn rows(bank: &Payload) -> Vec<usize> {
    bank.array(bank.pointer(24).unwrap() + 0x88, 56, Some(0x8080453B))
        .unwrap()
}

/// The body of the one row of `key` whose body has `class`.
fn find_body(bank: &Payload, key: u32, class: u32) -> (usize, usize) {
    let found = rows(bank)
        .into_iter()
        .filter(|&at| bank.u32(at + ROW_KEY).unwrap() == key)
        .map(|at| (at, bank.pointer(at + ROW_BODY).unwrap()))
        .filter(|&(_, body)| bank.u32(body + 4).unwrap() == class)
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "one {class:08X} row of {key:08X}");
    found[0]
}

fn gate(bank: &Payload, row: usize) -> u32 {
    if bank.u64(row + ROW_GATE).unwrap() == 0 {
        return 0;
    }
    let block = bank.pointer(row + ROW_GATE).unwrap();
    assert_eq!(bank.u32(block - 4).unwrap(), 0x8080454A);
    bank.u32(block).unwrap()
}

/// A record's one selected profile and its damage profile tag, through its contact block.
fn damage(bank: &Payload, record: usize) -> (usize, u32) {
    assert_eq!(bank.u32(record - 4).unwrap(), 0x80804464);
    let profile = bank.array(record + 72, 544, Some(0x80802D7B)).unwrap()[0];
    let contact = bank.pointer(profile + 0x1A8).unwrap();
    assert_eq!(bank.u32(contact - 4).unwrap(), 0x80804455);
    (profile, bank.u32(contact + 40).unwrap())
}

/// Returns each checked row for the artifact.
pub(super) fn readback(
    source: &Source,
    provenance: &serde_json::Value,
    (stock, staged): (&PackageManager, &PackageManager),
) -> serde_json::Value {
    let records = provenance["grid"].as_array().unwrap();
    let base = records
        .iter()
        .find(|record| record["placement"] == "melee_base")
        .expect("the grid has a melee foundation");
    let grants = |record: &serde_json::Value| {
        record["lowering"]["ability_grants"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    // Every key the foundation's rows apply, any of which upgrade rows may gate on.
    let foundation = grants(base)
        .iter()
        .map(|grant| u32::try_from(grant["property_key"].as_u64().unwrap()).unwrap())
        .collect::<BTreeSet<_>>();
    let mut checked = Vec::new();
    for record in records
        .iter()
        .filter(|record| record["placement"] == "melee")
    {
        for grant in grants(base).iter().chain(&grants(record)) {
            checked.push(read_grant(
                source,
                record,
                grant,
                (stock, staged),
                &foundation,
            ));
        }
    }
    assert!(!checked.is_empty(), "the grid places a melee");
    serde_json::Value::Array(checked)
}

fn read_grant(
    source: &Source,
    record: &serde_json::Value,
    grant: &serde_json::Value,
    (stock, staged): (&PackageManager, &PackageManager),
    foundation: &BTreeSet<u32>,
) -> serde_json::Value {
    let key = u32::try_from(grant["property_key"].as_u64().unwrap()).unwrap();
    let source_key = u32::try_from(grant["source_key"].as_u64().unwrap()).unwrap();
    let bank_tag = u32::try_from(grant["native_bank"].as_u64().unwrap()).unwrap();
    let staged_bank = Payload(staged.read_tag(TagHash(bank_tag)).unwrap());
    let stock_bank = Payload(stock.read_tag(TagHash(bank_tag)).unwrap());
    let source_bank = source
        .read(u32::try_from(grant["source_bank"].as_u64().unwrap()).unwrap())
        .unwrap();
    let source_row = usize::try_from(grant["source_property_offset"].as_u64().unwrap()).unwrap();
    let source_body = source_bank.pointer(source_row + 0x20).unwrap();
    let source_class = source_bank.u32(source_body - 4).unwrap();
    let class = match source_class {
        0x80802254 => 0x80804534,
        0x8080241C => 0x80804536,
        0x8080206A => 0x80804530,
        0x80802650 => 0x80804532,
        0x80802000 => 0x8080442A,
        0x808025AC => 0x80804427,
        0x8080235E => 0x8080451B,
        0x80802974 => 0x80804516,
        other => panic!("Unexpected source melee row {other:08X}"),
    };
    let (row, body) = find_body(&staged_bank, key, class);
    // Upgrade rows gate on the foundation's private key. A foundation row is ungated.
    let staged_gate = gate(&staged_bank, row);
    if source_bank.u64(source_row + 0x18).unwrap() == 0 {
        assert_eq!(staged_gate, 0, "{key:08X} gained a gate");
    } else {
        assert!(
            foundation.contains(&staged_gate) && !foundation.contains(&key),
            "{key:08X} gates on {staged_gate:08X}, not a private foundation key"
        );
    }
    let source_priority = source_bank.u32(source_row + 8).unwrap();
    let priority = staged_bank.u32(row + ROW_PRIORITY).unwrap();
    if matches!(class, 0x80804427 | 0x8080451B) {
        assert_eq!(priority, source_priority, "{key:08X} priority");
    } else {
        assert_eq!(priority, 255);
    }
    let value = read_melee_value(
        (&source_bank, source_body),
        (&stock_bank, source_key),
        (&staged_bank, body),
        (stock, staged),
        (key, class),
        grant,
    );
    json!({"tree":record["name"],"entry":record["entries"][0],
        "source_key":format!("{source_key:08X}"),"key":format!("{key:08X}"),
        "class":format!("{class:08X}"),"priority":priority,
        "gate":format!("{staged_gate:08X}"),"value":value})
}

fn read_melee_value(
    (source_bank, source_body): (&Payload, usize),
    (stock_bank, source_key): (&Payload, u32),
    (staged_bank, body): (&Payload, usize),
    (stock, staged): (&PackageManager, &PackageManager),
    (key, class): (u32, u32),
    grant: &serde_json::Value,
) -> serde_json::Value {
    // Where Bungie's port stores the field under the same key, it agrees with the row.
    let stock_value = |offset: usize| {
        let (_, stock_body) = find_body(stock_bank, source_key, class);
        stock_bank.u32(stock_body + offset).unwrap()
    };
    match class {
        0x80804534 => {
            let value = staged_bank.u32(body + 16).unwrap();
            assert_eq!(value, source_bank.u32(source_body).unwrap());
            json!(value)
        }
        0x80804536 => {
            let value = staged_bank.u32(body + 16).unwrap();
            assert_eq!(value, stock_value(16), "flag word against the native port");
            assert_eq!(
                u32::from(source_bank.u16(source_body).unwrap()) >> 1,
                value,
                "flag bits sit one place lower"
            );
            json!(format!("{value:04X}"))
        }
        0x80804530 => {
            let value = staged_bank.u8(body + 16).unwrap();
            assert_eq!(value, source_bank.u8(source_body).unwrap());
            assert_eq!(u32::from(value), stock_value(16) & 0xFF);
            json!(value)
        }
        0x80804532 => {
            let value = staged_bank.u32(body + 16).unwrap();
            assert_eq!(value, source_bank.u32(source_body).unwrap());
            let table = staged
                .read_tag(TagHash(
                    sundial::package_authoring::ability_hud::GLYPH_TABLE,
                ))
                .unwrap();
            assert!(
                table[48..]
                    .chunks_exact(112)
                    .any(|row| row[..4] == value.to_le_bytes()),
                "HUD name {value:08X} has a glyph row"
            );
            json!(format!("{value:08X}"))
        }
        0x8080442A => {
            assert_eq!(
                staged_bank.bytes::<8>(body + 16).unwrap(),
                source_bank.bytes::<8>(source_body).unwrap()
            );
            let entity = staged_bank.u32(body + 32).unwrap();
            assert_eq!(
                entity,
                stock_value(32),
                "the contact keeps the native entity"
            );
            json!({"native_entity":format!("{entity:08X}"),
                "source_entity":format!("{:08X}", source_bank.u32(source_body + 16).unwrap())})
        }
        0x80804427 => {
            assert_eq!(
                staged_bank.bytes::<8>(body + 16).unwrap(),
                source_bank.bytes::<8>(source_body).unwrap()
            );
            let amounts = check_damage_profiles(staged_bank, body, (stock, staged), grant, key);
            json!(amounts)
        }
        0x8080451B => read_script_parameter((source_bank, source_body), (staged_bank, body), key),
        _ => json!(null),
    }
}

fn check_damage_profiles(
    staged_bank: &Payload,
    body: usize,
    (stock, staged): (&PackageManager, &PackageManager),
    grant: &serde_json::Value,
    key: u32,
) -> Vec<f32> {
    let mut amounts = Vec::new();
    for (lane, field) in [0x18, 0x20].into_iter().enumerate() {
        let record = staged_bank.pointer(body + field).unwrap();
        let (profile_at, tag) = damage(staged_bank, record);
        // The source distances, not Shadowkeep's shorter ones: the profile's reach
        // and range, its lunge's range and next lane, and its contact control.
        let lunges = staged_bank
            .array(profile_at + 0x118, 8, Some(0x80804446))
            .unwrap();
        assert_eq!(lunges.len(), 1);
        let lunge = staged_bank.pointer(lunges[0]).unwrap();
        assert_eq!(staged_bank.u32(lunge - 4).unwrap(), 0x80804445);
        let contact = staged_bank.pointer(profile_at + 0x1A8).unwrap();
        let distances = [
            profile_at + 0x2C,
            profile_at + 0x204,
            lunge + 0x20,
            lunge + 0x30,
            contact + 8,
        ]
        .map(|at| staged_bank.f32(at).unwrap());
        let source = grant["attacks"][lane]["source_controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap() as f32)
            .collect::<Vec<_>>();
        assert_eq!(distances.to_vec(), source, "{key:08X} distances");
        assert!(
            stock.get_entry(TagHash(tag)).is_none(),
            "{key:08X} keeps a stock damage profile"
        );
        let payload = staged.read_tag(TagHash(tag)).unwrap();
        let profile = sundial::package_authoring::ability_damage::profile(tag, &payload)
            .expect("a damage profile");
        let amount = f32::from_le_bytes(
            payload[profile.root + 0x14..profile.root + 0x18]
                .try_into()
                .unwrap(),
        );
        let expected = grant["attacks"][lane]["source_profile_lower"]
            .as_f64()
            .unwrap();
        assert!(
            (f64::from(amount) - expected).abs() < 1e-4,
            "{key:08X} damage {amount} against the source {expected}"
        );
        amounts.push(amount);
    }
    amounts
}

fn read_script_parameter(
    (source_bank, source_body): (&Payload, usize),
    (staged_bank, body): (&Payload, usize),
    key: u32,
) -> serde_json::Value {
    let declarations = source_bank.array(source_body, 8, Some(0x80802AA5)).unwrap();
    assert_eq!(declarations.len(), 1);
    let declaration = declarations[0];
    let (input, operation) = (
        source_bank.u8(declaration).unwrap(),
        source_bank.u8(declaration + 1).unwrap(),
    );
    let change = source_bank.f32(declaration + 4).unwrap();
    let expected = if operation == 0 {
        change
    } else if change > 0.0 {
        1.0 + change
    } else {
        1.0 / (1.0 - change)
    };
    let child = staged_bank.pointer(body + 24).unwrap() + 16;
    assert_eq!(staged_bank.u8(child + 16).unwrap(), input);
    assert_eq!(staged_bank.u8(child + 17).unwrap(), operation);
    let factor = staged_bank.f32(child + 20).unwrap();
    assert!(
        (factor - expected).abs() < 1e-5,
        "{key:08X} factor {factor} against the source {expected}"
    );
    json!({"input":input,"multiply":operation != 0,"factor":factor})
}
