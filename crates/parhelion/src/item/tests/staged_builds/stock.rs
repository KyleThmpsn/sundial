//! Stock fixtures shared by native and import package workflows.
use super::*;

/// Find the bank through the copied entity's component records and the original bank's native
/// definition class. Stock bank IDs cannot identify a freshly allocated private bank.
pub(super) fn copied_bank(manager: &PackageManager, original: u32, copied: u32) -> u32 {
    let source = manager.read_tag(TagHash(original)).unwrap();
    let bank = sundial::package_authoring::ability_modifier::entity_bank(&source)
        .unwrap()
        .unwrap();
    let source_bank = manager.read_tag(TagHash(bank)).unwrap();
    let definition = relative_target(&source_bank, 0x18).unwrap();
    let class = read_u32(&source_bank, definition + 4).unwrap();
    let entity = manager.read_tag(TagHash(copied)).unwrap();
    let (count, _, rows, component_class) = array_at(&entity, 0x10).unwrap();
    assert_eq!(component_class, 0x80809C04);
    let mut banks = std::collections::BTreeSet::new();
    for row in 0..count {
        let owner = read_u32(&entity, rows + row * 12).unwrap();
        let payload = manager.read_tag(TagHash(owner)).unwrap();
        if let Ok(definition) = relative_target(&payload, 0x18)
            && read_u32(&payload, definition + 4).ok() == Some(class)
        {
            banks.insert(owner);
        }
    }
    assert_eq!(
        banks.len(),
        1,
        "the copied entity names one bank of the source class"
    );
    *banks.first().unwrap()
}

pub(super) fn runtime_plug(manager: &PackageManager, item_hash: u32) -> (usize, u32, u16, usize) {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap();
    let item_table = manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let (item_count, _, rows, _) = array_at(&item_table, 8).unwrap();
    let sword_row = find_u32_row_key(&item_table, rows, item_count, ITEM_ROW_SIZE, item_hash)
        .unwrap()
        .unwrap();
    let sword_definition = manager
        .read_tag(TagHash(
            read_u32(&item_table, rows + sword_row * ITEM_ROW_SIZE + 16).unwrap(),
        ))
        .unwrap();
    let choices = weapon_default_plug_indices(&sword_definition).unwrap();
    for (socket, choice) in choices.iter().enumerate().skip(4) {
        let index = usize::from(*choice);
        if index >= item_count {
            continue;
        }
        let row = rows + index * ITEM_ROW_SIZE;
        let plug_hash = read_u32(&item_table, row).unwrap();
        let definition = manager
            .read_tag(TagHash(read_u32(&item_table, row + 16).unwrap()))
            .unwrap();
        for perk in weapon_sandbox_perks(&definition).unwrap_or_default() {
            if sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action(
                manager,
                &globals,
                usize::from(perk),
            )
            .is_ok()
            {
                return (socket, plug_hash, perk, choices.len());
            }
        }
    }
    panic!("The clean donor has no trait plug with a runtime perk")
}
