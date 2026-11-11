//! Gated rows. A source row gated on another key applies only while that key applies too, so a
//! placed step with one sits in a tree whose melee applies the gate, and a step kept native for
//! its gate sits in a tree whose melee does not. A placed row's staged copy points at a gate block
//! naming the key and selects the same parameter rows the source row does. A gated row beside a
//! melee without its gate, or one selecting other parameters, is a failure.
use super::*;

/// Keys the entry's own custom perks apply.
fn program_keys(recipe: &crate::WeaponRecipe, entry: u8) -> BTreeSet<u32> {
    use sundial::package_authoring::sandbox_perk::program::{Action, PropertyOperation};
    let Some(abilities) = recipe.overrides.subclass_abilities.as_ref() else {
        return BTreeSet::new();
    };
    let edits = abilities
        .choices
        .iter()
        .find(|choice| choice.entry == entry)
        .map(|choice| &choice.edits)
        .or_else(|| {
            AttunementPath::ALL.into_iter().find_map(|path| {
                let position = path.entries().iter().position(|&own| own == entry)?;
                abilities
                    .attunement(path)?
                    .node(u8::try_from(position).unwrap())
                    .map(|node| &node.edits)
            })
        });
    edits
        .into_iter()
        .flat_map(|edits| &edits.custom_perks)
        .flat_map(|perk| &perk.effects)
        .filter_map(|effect| effect.program.as_ref())
        .flat_map(|program| &program.actions)
        .filter_map(|action| match action {
            Action::AbilityProperty {
                key,
                option: PropertyOperation::Apply,
                ..
            } => Some(*key),
            _ => None,
        })
        .collect()
}

/// Returns each checked row for the artifact.
pub(super) fn readback(
    source: &Source,
    provenance: &serde_json::Value,
    staged: &PackageManager,
    subclass: &sundial::investment::SubclassSummary,
    recipe: &crate::WeaponRecipe,
) -> serde_json::Value {
    let mut checked = Vec::new();
    for record in provenance["grid"].as_array().unwrap() {
        let gated = record["lowering"]["ability_grants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|grant| grant["gate"].is_u64())
            .collect::<Vec<_>>();
        let unapplied = record["unapplied_gates"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|gate| u32::try_from(gate.as_u64().unwrap()).unwrap())
            .collect::<BTreeSet<_>>();
        if gated.is_empty() && unapplied.is_empty() {
            continue;
        }
        let entry = u8::try_from(record["entries"][0].as_u64().unwrap()).unwrap();
        let path = AttunementPath::ALL
            .into_iter()
            .find(|path| path.entries().contains(&entry))
            .expect("a gated step sits in a tree");
        let melee = path.entries()[usize::from(path == AttunementPath::Middle)];
        // A native melee applies its stock keys, and a placed one the keys its programs apply.
        let mut applied = subclass
            .entry_modifiers
            .get(&melee)
            .into_iter()
            .flatten()
            .map(|&(key, _)| key)
            .collect::<BTreeSet<_>>();
        applied.extend(program_keys(recipe, melee));
        for gate in &unapplied {
            assert!(
                !applied.contains(gate),
                "{} stays native although its tree applies {gate:08X}",
                record["name"]
            );
        }
        if record["placement"] == "native" {
            assert!(!unapplied.is_empty() || !record["unsupported"].as_array().unwrap().is_empty());
            continue;
        }
        for grant in gated {
            let gate = u32::try_from(grant["gate"].as_u64().unwrap()).unwrap();
            assert!(
                applied.contains(&gate),
                "{} is placed beside a melee without its gate {gate:08X}",
                record["name"]
            );
            let key = u32::try_from(grant["property_key"].as_u64().unwrap()).unwrap();
            let bank_tag = u32::try_from(grant["native_bank"].as_u64().unwrap()).unwrap();
            let bank = Payload(staged.read_tag(TagHash(bank_tag)).unwrap());
            let definition =
                usize::try_from(bank.u64(bank.pointer(16).unwrap() + 8).unwrap()).unwrap();
            let row = bank
                .array(definition + 0x88, 56, Some(0x8080453B))
                .unwrap()
                .into_iter()
                .find(|&at| bank.u32(at + 0x10).unwrap() == key)
                .expect("the staged bank holds the gated row");
            let block = bank.pointer(row + 0x28).unwrap();
            assert_eq!(bank.u32(block - 4).unwrap(), 0x8080454A);
            assert_eq!(
                bank.u32(block).unwrap(),
                gate,
                "the row's gate block names its gate"
            );
            // The source row selects its parameters by index into its own bank's table. The
            // staged row selects rows of the same names and values in the native table.
            let source_bank = source
                .read(u32::try_from(grant["source_bank"].as_u64().unwrap()).unwrap())
                .unwrap();
            let at = usize::try_from(grant["source_property_offset"].as_u64().unwrap()).unwrap();
            let source_body = source_bank.pointer(at + 0x20).unwrap();
            // Numeric rows and attack containers are read back with the rest of their melee.
            if source_bank.u32(source_body - 4).unwrap() != 0x80802974 {
                checked.push(json!({"step":record["name"],"entry":entry,"melee":melee,
                    "gate":format!("{gate:08X}"),"bank":format!("{bank_tag:08X}"),
                    "key":format!("{key:08X}")}));
                continue;
            }
            let body = bank.pointer(row + 0x30).unwrap();
            assert_eq!(bank.u32(body + 4).unwrap(), 0x80804516);
            let selected = |payload: &Payload, table: usize, class: u32, at: usize| {
                let rows = payload.array(table, 16, Some(class)).unwrap();
                let first = usize::try_from(payload.u32(at).unwrap()).unwrap();
                let count = usize::try_from(payload.u32(at + 4).unwrap()).unwrap();
                rows[first..first + count]
                    .iter()
                    .map(|&row| payload.bytes::<16>(row).unwrap())
                    .collect::<Vec<_>>()
            };
            let source_parameters = selected(
                &source_bank,
                source_bank.pointer(24).unwrap() + 0xB0,
                0x80802122,
                source_body + 0x10,
            );
            assert!(!source_parameters.is_empty());
            assert_eq!(
                selected(&bank, definition + 0x98, 0x80804519, body + 0x20),
                source_parameters,
                "{} selects other parameters",
                record["name"]
            );
            checked.push(json!({"step":record["name"],"entry":entry,"melee":melee,
                "gate":format!("{gate:08X}"),"bank":format!("{bank_tag:08X}"),
                "key":format!("{key:08X}"),
                "parameters":source_parameters.iter().map(hex::encode).collect::<Vec<_>>()}));
        }
    }
    serde_json::Value::Array(checked)
}
