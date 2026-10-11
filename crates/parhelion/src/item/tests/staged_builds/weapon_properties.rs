//! Change Weapon Properties: Crimson's Banned Weapon entity attached to the weapon itself with
//! every modifier row neutral, and the rows an author sets, through the private perk copy and
//! through a weapon built, staged and read back from the staged packages. The ways this could be
//! wrong, each checked against the packages:
//!
//! - the stock perks whose attachments carry these rows attach them to the player, not the item
//! - the template carries components besides its modifiers, so the attachment does more than
//!   its rows
//! - a row is missing from the neutral set, so a stock amount survives into the copy
//! - an author's amount, or a row retargeted to another input, lands on the wrong record, or an
//!   untouched row moves
//! - the private action attaches the stock entity, or attaches the copy to the player
//! - the staged weapon's private perk fires the stock entity, or its copy reads differently
//!   from the clone, or the stock entity changes in the staged packages
//! - an added row is missing from the copy's array, lands on the wrong input, or displaces a
//!   stock row, or the array's descriptor still counts the stock rows
use super::*;
use crate::app::{WeaponProperties, WeaponPropertyPart as Part, weapon_properties};
use crate::recipe::{
    HexHash, WeaponRecipe, WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe,
};
use sundial::package_authoring::{
    runtime::{
        WeaponRuntimeValue, WeaponRuntimeValueOverride, load_weapon_runtime_graph_for_entity,
        modifiers,
    },
    sandbox_perk::{
        self, action,
        entity::modifiers as records,
        program::{Action, Asset, AttachmentTarget, ModifierRow, Program, Trigger},
    },
};

const TEMPLATE: u32 = 0x80EF_AE52;
const BLACK_HOLE_ATTACHMENT: u32 = 0x80EF_ADD5;
const WEAPON_CONTROLLER: i64 = 0;
const SPREAD: i64 = 39;
const RANGE: i64 = 9;

/// The rows an author sets on the template: bullets per shot doubled instead of added to, triple
/// damage, and a
/// Weapon Controller row retargeted to the Barrel's Spread, which the template has no row for,
/// multiplied by 2.5. Then two rows added: Spread multiplied by 2 and three more magazine rounds.
struct Authored {
    properties: WeaponProperties,
    values: Vec<WeaponRuntimeValueOverride>,
    added: Vec<ModifierRow>,
    burst: usize,
    damage: usize,
    retargeted: usize,
}

impl Authored {
    fn read(packages: &Path) -> Self {
        let properties = weapon_properties(packages, TEMPLATE).unwrap();
        let rows = &properties.rows;
        let row_for = |component: i64, input: i64| {
            rows.iter()
                .position(|(_, row)| {
                    row.component_number() == component && row.input_number() == input
                })
                .unwrap_or_else(|| panic!("a row for component {component} input {input}"))
        };
        let burst = row_for(
            modifiers::BARREL_COMPONENT,
            modifiers::BARREL_BULLETS_PER_SHOT[0],
        );
        let damage = row_for(modifiers::BARREL_COMPONENT, 32);
        let retargeted = row_for(WEAPON_CONTROLLER, RANGE);
        assert!(
            rows[burst].1.adds() && rows[damage].1.multiplies() && rows[retargeted].1.multiplies()
        );
        let mut values = properties.neutral.clone();
        properties
            .set(burst, Part::Amount, float(2.0), &mut values)
            .unwrap();
        let operation = properties.value(burst, Part::Operation, &values).unwrap();
        properties
            .set(
                burst,
                Part::Operation,
                renumber(operation, modifiers::OPERATION_MULTIPLY),
                &mut values,
            )
            .unwrap();
        properties
            .set(damage, Part::Amount, float(3.0), &mut values)
            .unwrap();
        let component = properties
            .value(retargeted, Part::Component, &values)
            .unwrap();
        let input = properties.value(retargeted, Part::Input, &values).unwrap();
        properties
            .set(
                retargeted,
                Part::Component,
                renumber(component, modifiers::BARREL_COMPONENT),
                &mut values,
            )
            .unwrap();
        properties
            .set(
                retargeted,
                Part::Input,
                renumber(input, SPREAD),
                &mut values,
            )
            .unwrap();
        properties
            .set(retargeted, Part::Amount, float(2.5), &mut values)
            .unwrap();
        let added = vec![
            ModifierRow {
                component: modifiers::BARREL_COMPONENT as u8,
                ability: -1,
                input: SPREAD as i16,
                operation: modifiers::OPERATION_MULTIPLY as u8,
                amount_bits: 2.0_f32.to_bits(),
            },
            ModifierRow {
                component: modifiers::MAGAZINE_COMPONENT as u8,
                ability: -1,
                input: 0,
                operation: modifiers::OPERATION_ADD as u8,
                amount_bits: 3.0_f32.to_bits(),
            },
        ];
        Self {
            properties,
            values,
            added,
            burst,
            damage,
            retargeted,
        }
    }

    /// How many rows the copy holds: the stock rows and the added ones.
    fn count(&self) -> usize {
        self.properties.rows.len() + self.added.len()
    }

    /// The amount row `index` holds once built.
    fn amount(&self, index: usize) -> f32 {
        if let Some(added) = index.checked_sub(self.properties.rows.len()) {
            return self.added[added].amount();
        }
        if index == self.burst {
            2.0
        } else if index == self.damage {
            3.0
        } else if index == self.retargeted {
            2.5
        } else {
            self.properties.rows[index].1.neutral()
        }
    }

    /// The operation row `index` applies once built.
    fn operation(&self, index: usize) -> i64 {
        if let Some(added) = index.checked_sub(self.properties.rows.len()) {
            return i64::from(self.added[added].operation);
        }
        if index == self.burst {
            modifiers::OPERATION_MULTIPLY
        } else {
            self.properties.rows[index].1.operation_byte()
        }
    }

    /// The component and input row `index` addresses once built.
    fn target(&self, index: usize) -> (i64, i64) {
        if let Some(added) = index.checked_sub(self.properties.rows.len()) {
            let row = &self.added[added];
            return (i64::from(row.component), i64::from(row.input));
        }
        let row = &self.properties.rows[index].1;
        if index == self.retargeted {
            (modifiers::BARREL_COMPONENT, SPREAD)
        } else {
            (row.component_number(), row.input_number())
        }
    }

    /// The attach action, as the picker's row builds it.
    fn attach(&self) -> Action {
        Action::Attach {
            asset: Asset {
                graph: TEMPLATE,
                values: self.values.clone(),
                rows: self.added.clone(),
                ..Asset::default()
            },
            mode: AttachmentTarget::ThisItem,
            keys: [0; 2],
            float_bits: [0; 4],
        }
    }
}

fn float(value: f32) -> WeaponRuntimeValue {
    WeaponRuntimeValue::Float32Bits(value.to_bits())
}

fn renumber(value: WeaponRuntimeValue, number: i64) -> WeaponRuntimeValue {
    match value {
        WeaponRuntimeValue::Signed(_) => WeaponRuntimeValue::Signed(number),
        WeaponRuntimeValue::Unsigned(_) => WeaponRuntimeValue::Unsigned(number as u64),
        other => panic!("a number, not {other:?}"),
    }
}

/// The stock perks whose attachments carry such rows attach them to the item itself.
fn assert_stock_perks_attach_to_the_item(manager: &PackageManager, globals: &[u8]) {
    for (perk, entity) in [(502, BLACK_HOLE_ATTACHMENT), (662, TEMPLATE)] {
        let runtime = load_sandbox_perk_runtime_action(manager, globals, perk).unwrap();
        let decoded = action::decode(&runtime.action_payload).unwrap();
        let attach = decoded
            .effects()
            .find(|effect| effect.kind == 1 && effect.referenced_tag == Some(entity))
            .unwrap_or_else(|| panic!("perk {perk} attaches 0x{entity:08X}"));
        assert_eq!(
            attach.native[2],
            AttachmentTarget::ThisItem.byte(),
            "perk {perk} attaches 0x{entity:08X} to the item itself"
        );
    }
}

/// Every record of the template's private copy at `root`, read through `manager`, holds its
/// neutral amount or the amount the author set, and addresses the component and input the
/// author chose.
fn assert_rows_built(manager: &PackageManager, root: u32, authored: &Authored, route: &str) {
    let payload = manager.read_tag(TagHash(root)).unwrap();
    let mut graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, root, &payload).unwrap();
    graph.scope_fields();
    let rows = records::discover(&graph);
    if rows.len() != authored.count() {
        for resource in &graph.resources {
            println!(
                "{route}: resource {} 0x{:08X} owner 0x{:08X}: instance {} fields, issues {:?}; definition {:?} fields, issues {:?}",
                resource.binding_label,
                resource.concrete_class,
                resource.owner_tag,
                resource.instance.fields.len(),
                resource.instance.structure.issues,
                resource.definition.as_ref().map(|root| root.fields.len()),
                resource
                    .definition
                    .as_ref()
                    .map(|root| &root.structure.issues),
            );
        }
        let stock_records = authored
            .properties
            .rows
            .iter()
            .map(|(_, row)| row.clone())
            .collect::<Vec<_>>();
        let stock_owner = manager.read_tag(TagHash(stock_records[0].owner_tag));
        let first = &stock_records[0];
        let binding = first.amount.locator.binding_hash.get();
        let index = usize::from(first.amount.locator.resource_index);
        let owner_tag =
            sundial::package_authoring::entity::weapon_component_bindings(&payload, binding)
                .unwrap()[index]
                .owner_tag;
        let owner = manager.read_tag(TagHash(owner_tag)).unwrap();
        println!(
            "{route}: copied owner 0x{owner_tag:08X} is {} bytes, stock owner readable {}",
            owner.len(),
            stock_owner.is_ok()
        );
        if let Ok(array) = records::record_array(&owner, &stock_records) {
            println!("{route}: the copy still has the stock array at {array:?}");
        }
        for descriptor in (0..owner.len().saturating_sub(16)).step_by(8) {
            if let Ok((count, header, rows_at, class)) =
                sundial::package_authoring::native_payload::native_array_at(&owner, descriptor)
                && class == modifiers::SETTINGS_SCHEMA
            {
                println!(
                    "{route}: array descriptor +0x{descriptor:X}: {count} records, header +0x{header:X}, rows +0x{rows_at:X}"
                );
            }
        }
    }
    assert_eq!(rows.len(), authored.count(), "{route}: every row is copied");
    // The copy's array is the appended one, after the stock array, and the records are one
    // contiguous array in it.
    let stock_end =
        authored.properties.rows.last().unwrap().1.owner_offset as usize + records::RECORD_SIZE;
    assert!(
        rows[0].owner_offset as usize >= stock_end,
        "{route}: the copy's rows follow the stock array"
    );
    assert!(rows.iter().all(|row| row.owner_tag == rows[0].owner_tag));
    for (index, row) in rows.iter().enumerate() {
        let (component, input) = authored.target(index);
        assert_eq!(
            row.stock(),
            authored.amount(index),
            "{route}: row {index} component {component} input {input} amount"
        );
        assert_eq!(
            row.component_number(),
            component,
            "{route}: row {index} component"
        );
        assert_eq!(row.input_number(), input, "{route}: row {index} input");
        assert_eq!(
            row.operation_byte(),
            authored.operation(index),
            "{route}: row {index} operation"
        );
        assert_eq!(
            row.owner_offset as usize,
            rows[0].owner_offset as usize + index * records::RECORD_SIZE,
            "{route}: row {index} is in place"
        );
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn change_weapon_properties_attaches_neutral_rows_and_the_rows_an_author_sets() {
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    assert_stock_perks_attach_to_the_item(&manager, &globals);
    let authored = Authored::read(&packages);
    let properties = &authored.properties;
    println!(
        "template components:\n{}",
        properties.components().join("\n")
    );
    let rows = &properties.rows;
    for (index, (_, row)) in rows.iter().enumerate() {
        println!(
            "row {index}: owner 0x{:08X} +0x{:X} component {} input {} operation {} amount {}",
            row.owner_tag,
            row.owner_offset,
            row.component_number(),
            row.input_number(),
            row.operation_byte(),
            row.stock()
        );
    }
    // The template is the modifiers and nothing else.
    assert!(
        properties.components().iter().all(|line| {
            line.contains("0x80803B00") || line.contains("0x80803B05") || line.starts_with("owner ")
        }),
        "{:?}",
        properties.components()
    );
    assert_eq!(
        rows.iter()
            .map(|(_, row)| row.component_number())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            WEAPON_CONTROLLER,
            modifiers::MAGAZINE_COMPONENT,
            modifiers::BARREL_COMPONENT
        ])
    );
    assert!(rows.len() >= 21, "{} rows", rows.len());
    // Every row reads neutral once the neutral set is applied.
    for (index, (_, row)) in rows.iter().enumerate() {
        assert_eq!(
            properties
                .value(index, Part::Amount, &properties.neutral)
                .unwrap(),
            float(row.neutral()),
            "row {index}"
        );
    }
    // The attachment on a stock action, as the scoping test builds one.
    let mut stock = load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
    let mut unset = authored.attach();
    if let Action::Attach { asset, .. } = &mut unset {
        asset.values.clear();
    }
    let compiled = sandbox_perk::program::compile(
        &manager,
        &Program {
            trigger: Trigger::Always,
            actions: vec![unset],
            ..Program::default()
        },
    )
    .unwrap();
    stock.action_payload = compiled.payload;
    let template_payload = manager.read_tag(TagHash(TEMPLATE)).unwrap();
    stock.graphs = vec![sandbox_perk::SandboxPerkRuntimeGraphSource {
        tag: TagHash(TEMPLATE),
        action_offsets: vec![compiled.graph_offsets[0].unwrap()],
        payload: template_payload.clone(),
    }];
    let allocator = AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0);
    let mut tags = Vec::new();
    let typed = Program {
        trigger: Trigger::Always,
        actions: vec![authored.attach()],
        ..Program::default()
    };
    clone_private_sandbox_perk_runtime(
        &manager,
        &stock,
        custom_runtime::PrivateRuntimeEdits {
            program: Some(&typed),
            ..Default::default()
        },
        allocator,
        &mut tags,
    )
    .unwrap();
    let ordinal = tags
        .iter()
        .position(|tag| tag.template_tag == TagHash(TEMPLATE))
        .expect("the template is copied");
    let private = allocator.assigned_tag(ordinal, "test", "graph").unwrap();
    // Every row of the private owner holds its neutral amount, or the amount the author set, at
    // the record's own bytes, while the stock owner keeps its own. The copy keeps the stock
    // layout, so each record is read where the stock one is.
    let owner = rows[0].1.owner_tag;
    assert!(rows.iter().all(|(_, row)| row.owner_tag == owner));
    let stock_owner = manager.read_tag(TagHash(owner)).unwrap();
    let owner_copy = &tags
        .iter()
        .find(|tag| tag.template_tag == TagHash(owner))
        .expect("the owner is copied")
        .payload;
    assert!(
        owner_copy.len() > stock_owner.len(),
        "the owner grows by the appended array"
    );
    let amount = |payload: &[u8], at: usize| {
        let at = at + modifiers::AMOUNT_OFFSET as usize;
        f32::from_le_bytes(payload[at..at + 4].try_into().unwrap())
    };
    let operation =
        |payload: &[u8], at: usize| i64::from(payload[at + modifiers::OPERATION_OFFSET as usize]);
    let input = |payload: &[u8], at: usize| {
        let at = at + modifiers::INPUT_OFFSET as usize;
        i64::from(i16::from_le_bytes(payload[at..at + 2].try_into().unwrap()))
    };
    let component =
        |payload: &[u8], at: usize| i64::from(payload[at + modifiers::COMPONENT_OFFSET as usize]);
    for (index, (_, row)) in rows.iter().enumerate() {
        let at = row.owner_offset as usize;
        assert_eq!(
            amount(&stock_owner, at),
            row.stock(),
            "row {index} stock amount"
        );
        assert_eq!(
            component(&stock_owner, at),
            row.component_number(),
            "row {index} stock component"
        );
        assert_eq!(
            input(&stock_owner, at),
            row.input_number(),
            "row {index} stock input"
        );
        let (expected_component, expected_input) = authored.target(index);
        assert_eq!(
            amount(owner_copy, at),
            authored.amount(index),
            "row {index}: component {} input {}",
            row.component_number(),
            row.input_number()
        );
        assert_eq!(
            component(owner_copy, at),
            expected_component,
            "row {index} component"
        );
        assert_eq!(input(owner_copy, at), expected_input, "row {index} input");
        assert_eq!(
            operation(owner_copy, at),
            authored.operation(index),
            "row {index} operation"
        );
        assert_eq!(
            operation(&stock_owner, at),
            row.operation_byte(),
            "row {index} stock operation"
        );
    }
    // The copy's descriptor counts every row and points at the appended array, where the stock
    // rows read as the values leave them and the added rows follow.
    let stock_records = rows.iter().map(|(_, row)| row.clone()).collect::<Vec<_>>();
    let array = records::record_array(&stock_owner, &stock_records).unwrap();
    let (count, header, appended, class) = array_at(owner_copy, array.descriptor).unwrap();
    assert_eq!(count, authored.count());
    assert_eq!(class, modifiers::SETTINGS_SCHEMA);
    assert!(appended >= stock_owner.len(), "the array is appended");
    assert_eq!(header % 16, 0, "the appended header is aligned");
    assert_eq!(
        read_u32(owner_copy, header - 4).unwrap(),
        0x8080_9FBD,
        "the typed-array marker precedes the appended header"
    );
    for index in 0..count {
        let at = appended + index * records::RECORD_SIZE;
        let (expected_component, expected_input) = authored.target(index);
        assert_eq!(
            amount(owner_copy, at),
            authored.amount(index),
            "appended row {index} amount"
        );
        assert_eq!(
            component(owner_copy, at),
            expected_component,
            "appended row {index} component"
        );
        assert_eq!(
            input(owner_copy, at),
            expected_input,
            "appended row {index} input"
        );
        assert_eq!(
            operation(owner_copy, at),
            authored.operation(index),
            "appended row {index} operation"
        );
    }
    // The private action attaches the private copy to the item itself.
    let action = &tags
        .iter()
        .find(|tag| tag.template_tag == stock.action_tag)
        .expect("the action is copied")
        .payload;
    let decoded = action::decode(action).unwrap();
    let attach = decoded.effects().find(|effect| effect.kind == 1).unwrap();
    assert_eq!(attach.referenced_tag, Some(private.0));
    assert_eq!(attach.native[2], AttachmentTarget::ThisItem.byte());
    for source in &stock.graphs {
        assert_eq!(manager.read_tag(source.tag).unwrap(), source.payload);
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn change_weapon_properties_rows_reach_the_staged_packages() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("weapon-properties");
    fs::create_dir_all(&output).unwrap();
    let stock = open_shadowkeep_package_manager(&packages).unwrap();
    let authored = Authored::read(&packages);
    let (socket, plug, perk, sockets) = super::stock::runtime_plug(&stock, 0x0222_2CBF);
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.weapon-properties.e2e", 0x0222_2CBF, "")
            .unwrap();
    recipe.name = "Weapon Properties Fixture".into();
    recipe.overrides.socket_columns = vec![None; sockets];
    recipe.overrides.socket_columns[socket] = Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(plug)],
        ..Default::default()
    });
    let mut effect = crate::perk::PerkRecipe::effect(perk);
    effect.program = Some(Program {
        name: "Weapon Properties".into(),
        trigger: Trigger::Always,
        actions: vec![authored.attach()],
        ..Program::default()
    });
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
            socket_index: socket as u16,
            choice_index: 0,
            source_plug_hash: HexHash::new(plug),
            name: Some("Weapon Properties".into()),
            replace_effects: true,
            sandbox_perks: vec![effect],
            investment_stats: Vec::new(),
            classification_donor_hash: None,
            icon: None,
            description: None,
            offer_everywhere: false,
            additional_sandbox_perks: Vec::new(),
        });
    // The recipe survives a save and a reload with its rows.
    let saved = output.join("weapon-properties.parhelion.json");
    fs::write(&saved, recipe.to_json_pretty().unwrap()).unwrap();
    let reloaded = WeaponRecipe::load_json(&saved).unwrap();
    assert_eq!(
        reloaded.to_json_pretty().unwrap(),
        recipe.to_json_pretty().unwrap()
    );
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![reloaded.to_spec().unwrap()],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-weapon-properties-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let private = &bundle.plan.weapons[0].custom_plugs[0].perks[0];
    let runtime_map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let assignment = sandbox_perk_runtime_assignment(&runtime_map, private.runtime_key)
        .unwrap()
        .unwrap();
    let action_bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
    let decoded = action::decode(&action_bytes).unwrap();
    let attach = decoded
        .effects()
        .find(|effect| effect.kind == 1)
        .expect("the private perk attaches");
    assert_eq!(attach.native[2], AttachmentTarget::ThisItem.byte());
    let root = attach.referenced_tag.expect("an attached entity");
    assert_ne!(root, TEMPLATE, "the private perk attaches a copy");
    assert_eq!(
        staged.get_entry(TagHash(root)).unwrap().reference,
        0x8080_9C0F,
        "the copy is an entity"
    );
    assert_rows_built(&staged, root, &authored, "staged");
    // The stock entity and its owner are what every other weapon still uses.
    let owner = authored.properties.rows[0].1.owner_tag;
    for tag in [TEMPLATE, owner] {
        assert_eq!(
            staged.read_tag(TagHash(tag)).unwrap(),
            stock.read_tag(TagHash(tag)).unwrap(),
            "stock 0x{tag:08X} is unchanged"
        );
    }
    fs::write(output.join("action.bin"), &action_bytes).unwrap();
    fs::write(
        output.join("attachment.bin"),
        staged.read_tag(TagHash(root)).unwrap(),
    )
    .unwrap();
    crate::test_support::artifact(
        "weapon-properties/readback",
        &serde_json::json!({
            "recipe": saved,
            "source_packages": packages,
            "template": format!("{TEMPLATE:08X}"),
            "item_hash": bundle.plan.weapons[0].item_hash,
            "runtime_key": private.runtime_key,
            "action_tag": assignment.runtime_tag,
            "attached_entity": root,
            "rows": authored.properties.rows.iter().enumerate().map(|(index, (_, row))| {
                let (component, input) = authored.target(index);
                serde_json::json!({
                    "owner_offset": row.owner_offset,
                    "component": component,
                    "input": input,
                    "operation": row.operation_byte(),
                    "stock_amount": row.stock(),
                    "built_amount": authored.amount(index),
                })
            }).collect::<Vec<_>>(),
            "stock_unchanged": true,
            "installed": false,
            "gameplay_verified": false,
        }),
    );
}
