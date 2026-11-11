//! Source glide groups. Each placed glide choice and the glide foundation keep one private key
//! per source key, whose staged rows follow the source group in order: the fifteen glide factor
//! lanes and the impulse pair copied bit for bit, the horizontal burst as a constant program
//! returning the source factor, priority 255 and no gate. A choice without its own burst
//! returns the source glide controller's default instead. Each row takes its native witness's
//! storage and handler. The movement entries apply no stock key a private group translates, and
//! the stock rows of the bank keep their order and selection. A row that differs from its source,
//! a stock key still applied beside its private copy, or a moved stock row is a failure.
use super::*;
use parhelion_import::d1_roi::ability::SubclassCatalog;

const ROW_KEY: usize = 0x10;
const ROW_NAME: usize = 0x14;
const ROW_PRIORITY: usize = 0x18;
const ROW_HANDLER: usize = 0x1C;
const ROW_MODE: usize = 0x20;
const ROW_GATE: usize = 0x28;
const ROW_BODY: usize = 0x30;

fn rows(bank: &Payload) -> Vec<usize> {
    bank.array(bank.pointer(24).unwrap() + 0x88, 56, Some(0x8080453B))
        .unwrap()
}

/// Each row's selection and body class, with its gate key or zero.
fn selections(bank: &Payload) -> Vec<(u32, u32, u32, u32, u32, u32, u32)> {
    rows(bank)
        .into_iter()
        .map(|at| {
            let gate = if bank.u64(at + ROW_GATE).unwrap() == 0 {
                0
            } else {
                bank.u32(bank.pointer(at + ROW_GATE).unwrap()).unwrap()
            };
            let body = bank.pointer(at + ROW_BODY).unwrap();
            (
                bank.u32(at + ROW_KEY).unwrap(),
                bank.u32(at + ROW_NAME).unwrap(),
                bank.u32(at + ROW_PRIORITY).unwrap(),
                bank.u32(at + ROW_HANDLER).unwrap(),
                bank.u32(at + ROW_MODE).unwrap(),
                gate,
                bank.u32(body + 4).unwrap(),
            )
        })
        .collect()
}

/// Returns each checked group for the artifact.
pub(super) fn readback(
    source: &Source,
    subclass: &SubclassCatalog,
    provenance: &serde_json::Value,
    (stock, staged): (&PackageManager, &PackageManager),
    built: &sundial::investment::SubclassSummary,
) -> serde_json::Value {
    // The source glide controller's reset value for the horizontal burst, read from its
    // preparation defaults at definition +0x580 (factor at preparation +0x194).
    let defaults = subclass
        .abilities
        .iter()
        .filter_map(|ability| {
            let controller = source.read(ability.controller).ok()?;
            let definition = controller.pointer(24).ok()?;
            (controller.u32(definition.checked_sub(4)?).ok()? == 0x808027B4)
                .then(|| controller.u32(definition + 0x580 + 0x194).unwrap())
        })
        .collect::<BTreeSet<_>>();
    let mut groups = BTreeMap::<u32, Vec<&serde_json::Value>>::new();
    let mut entries = BTreeSet::new();
    let mut translated = BTreeSet::new();
    for record in provenance["grid"].as_array().unwrap() {
        if !matches!(
            record["placement"].as_str(),
            Some("movement" | "movement_base")
        ) {
            continue;
        }
        for entry in record["entries"].as_array().unwrap() {
            entries.insert(u8::try_from(entry.as_u64().unwrap()).unwrap());
        }
        for grant in record["lowering"]["ability_grants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|grant| grant["route"] == "source_glide_group")
        {
            let key = u32::try_from(grant["property_key"].as_u64().unwrap()).unwrap();
            translated.insert(u32::try_from(grant["source_key"].as_u64().unwrap()).unwrap());
            groups.entry(key).or_default().push(grant);
        }
    }
    assert!(
        !groups.is_empty(),
        "the source glide choices lower to private groups"
    );
    // No entry of the built subclass applies a stock key beside its private copy.
    for (entry, modifiers) in &built.entry_modifiers {
        for (key, _) in modifiers {
            assert!(
                !translated.contains(key),
                "entry {entry} still applies stock glide key {key:08X}"
            );
        }
    }
    let private = groups.keys().copied().collect::<BTreeSet<_>>();
    let checked = groups
        .iter()
        .map(|(&key, grants)| read_group(source, (stock, staged), &private, &defaults, key, grants))
        .collect::<Vec<_>>();
    serde_json::json!({"movement_entries":entries,"groups":checked})
}

fn read_group(
    source: &Source,
    (stock, staged): (&PackageManager, &PackageManager),
    private: &BTreeSet<u32>,
    defaults: &BTreeSet<u32>,
    key: u32,
    grants: &[&serde_json::Value],
) -> serde_json::Value {
    let bank_tag = u32::try_from(grants[0]["native_bank"].as_u64().unwrap()).unwrap();
    let staged_bank = Payload(staged.read_tag(TagHash(bank_tag)).unwrap());
    let stock_bank = Payload(stock.read_tag(TagHash(bank_tag)).unwrap());
    // Stock rows keep their order and selection around the private ones.
    assert_eq!(
        selections(&staged_bank)
            .into_iter()
            .filter(|row| !private.contains(&row.0))
            .collect::<Vec<_>>(),
        selections(&stock_bank),
        "the stock glide rows moved or changed"
    );
    let own = rows(&staged_bank)
        .into_iter()
        .filter(|&at| staged_bank.u32(at + ROW_KEY).unwrap() == key)
        .collect::<Vec<_>>();
    assert_eq!(
        own.len(),
        grants.len(),
        "the private group keeps every source row"
    );
    let source_bank = source
        .read(u32::try_from(grants[0]["source_bank"].as_u64().unwrap()).unwrap())
        .unwrap();
    let decoded = own
        .iter()
        .zip(grants)
        .map(|(&at, grant)| {
            read_row(
                (&source_bank, &stock_bank, &staged_bank),
                at,
                grant,
                defaults,
            )
        })
        .collect::<Vec<_>>();
    serde_json::json!({"key":format!("{key:08X}"),
            "source_key":format!("{:08X}", grants[0]["source_key"].as_u64().unwrap()),
            "bank":format!("{bank_tag:08X}"),"rows":decoded})
}

fn read_row(
    (source_bank, stock_bank, staged_bank): (&Payload, &Payload, &Payload),
    at: usize,
    grant: &serde_json::Value,
    defaults: &BTreeSet<u32>,
) -> serde_json::Value {
    let class = u32::try_from(grant["native_modifier_class"].as_u64().unwrap()).unwrap();
    let body = staged_bank.pointer(at + ROW_BODY).unwrap();
    assert_eq!(staged_bank.u32(body + 4).unwrap(), class);
    assert_eq!(staged_bank.u32(body - 4).unwrap(), class + 1);
    assert_eq!(staged_bank.u64(at + ROW_GATE).unwrap(), 0);
    assert_eq!(staged_bank.u32(at + ROW_PRIORITY).unwrap(), 255);
    assert_eq!(staged_bank.u32(at + ROW_MODE).unwrap(), 255);
    let template = usize::try_from(grant["native_template_offset"].as_u64().unwrap()).unwrap();
    assert_eq!(
        staged_bank.u32(at + ROW_HANDLER).unwrap(),
        stock_bank.u32(template + ROW_HANDLER).unwrap(),
        "the row takes its witness's handler"
    );
    assert_eq!(
        stock_bank
            .u32(stock_bank.pointer(template + ROW_BODY).unwrap() + 4)
            .unwrap(),
        class,
        "the witness stores the same class"
    );
    let source_words = |count: usize| {
        let offset = usize::try_from(grant["source_property_offset"].as_u64().unwrap()).unwrap();
        let source_body = source_bank.pointer(offset + 0x20).unwrap();
        assert_eq!(
            source_bank.u32(source_body - 4).unwrap(),
            u32::try_from(grant["source_modifier_class"].as_u64().unwrap()).unwrap()
        );
        (0..count)
            .map(|lane| source_bank.u32(source_body + lane * 4).unwrap())
            .collect::<Vec<_>>()
    };
    let words = read_words(
        staged_bank,
        body,
        class,
        source_words,
        grant["source_property_offset"].is_null(),
        defaults,
    );
    serde_json::json!({"class":format!("{class:08X}"),
            "handler":staged_bank.u32(at + ROW_HANDLER).unwrap(),
            "restored_default":grant["source_property_offset"].is_null(),
            "words":words.iter().map(|&word| f32::from_bits(word)).collect::<Vec<_>>()})
}

fn read_words(
    staged_bank: &Payload,
    body: usize,
    class: u32,
    source_words: impl Fn(usize) -> Vec<u32>,
    restored_default: bool,
    defaults: &BTreeSet<u32>,
) -> Vec<u32> {
    match class {
        0x80804258 | 0x808044D4 => {
            let count = if class == 0x80804258 { 15 } else { 2 };
            let staged_words = (0..count)
                .map(|lane| staged_bank.u32(body + 16 + lane * 4).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(staged_words, source_words(count), "lanes copy bit for bit");
            staged_words
        }
        0x808044CC => {
            assert_eq!(
                staged_bank.bytes::<4>(body + 0x68).unwrap(),
                [0x34, 0, 0x3E, 0],
                "a constant program"
            );
            assert_eq!(staged_bank.u64(body + 0x18).unwrap(), 4);
            assert_eq!(staged_bank.u64(body + 0x28).unwrap(), 1);
            let lanes = (0..4)
                .map(|lane| staged_bank.u32(body + 0x88 + lane * 4).unwrap())
                .collect::<Vec<_>>();
            assert!(lanes.iter().all(|&lane| lane == lanes[0]));
            let expected = if restored_default {
                assert_eq!(defaults.len(), 1, "one source glide default");
                *defaults.first().unwrap()
            } else {
                source_words(1)[0]
            };
            assert_eq!(lanes[0], expected, "the program returns the source factor");
            vec![lanes[0]]
        }
        other => panic!("Unexpected glide row class {other:08X}"),
    }
}
