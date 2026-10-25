use super::*;

struct Installed;
impl AccountCatalog for Installed {
    fn item(&self, _: u64) -> Option<&crate::catalog::ItemDef> {
        None
    }
    fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata> {
        const ORNAMENT: InventoryMetadata = InventoryMetadata {
            scope: InventoryScope::Profile,
            native_bucket_id: 13,
            stackability: ItemStackability::Stackable,
            max_stack_size: Some(999),
            bucket_capacity: Some(701),
        };
        const SHADER: InventoryMetadata = InventoryMetadata {
            native_bucket_id: 14,
            ..ORNAMENT
        };
        const MATERIAL: InventoryMetadata = InventoryMetadata {
            native_bucket_id: 15,
            ..ORNAMENT
        };
        const INSTANCED: InventoryMetadata = InventoryMetadata {
            stackability: ItemStackability::Instanced,
            ..ORNAMENT
        };
        const CHARACTER: InventoryMetadata = InventoryMetadata {
            scope: InventoryScope::Character,
            ..ORNAMENT
        };
        Some(match hash {
            1000..2000 => &ORNAMENT,
            2000..3000 => &SHADER,
            4000 => &INSTANCED,
            4001 => &CHARACTER,
            4002 => &SHADER,
            _ => &MATERIAL,
        })
    }
    fn contains_plug(&self, hash: u64) -> bool {
        hash != 4002
    }
}

fn dawn(hashes: &[u32]) -> (tempfile::TempDir, WorkspaceDocument) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&path);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("DELETE FROM profile_items", []).unwrap();
    for (position, hash) in hashes.iter().enumerate() {
        db.execute(
            "INSERT INTO profile_items VALUES(?1,'0000000000000000',?2,99,?3)",
            rusqlite::params![position as i64, hash, position as i64],
        )
        .unwrap();
    }
    let doc = WorkspaceDocument::load(
        json!({"version":6}),
        &temp.path().join("settings.json"),
        true,
    );
    assert!(doc.dawn_account().is_some());
    (temp, doc)
}

#[test]
fn app_edit_and_save_gates_reject_overflow_atomically() {
    let (_temp, mut doc) = dawn(&(1000..1100).collect::<Vec<_>>());
    let before = doc.clone();
    let hashes = (1000..1101).collect::<Vec<_>>();
    let catalog = Catalog::for_test_with_inventory(
        vec![crate::catalog::ItemDef {
            hash: 9999,
            name: "Socket source".into(),
            type_name: String::new(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: hashes
                .iter()
                .map(|hash| Some(format!("0x{hash:08X}")))
                .collect(),
            sockets: vec![],
            abilities: crate::catalog::AbilityOptions::default(),
        }],
        HashMap::new(),
        hashes
            .iter()
            .map(|hash| (*hash, *Installed.inventory_metadata(*hash).unwrap()))
            .collect(),
    );
    assert!(catalog.contains_plug(1100));
    let result = apply_with_bucket_limits(&mut doc, &catalog, |candidate| {
        account::add_profile_item(candidate, 1100, 1).map_err(|e| e.to_string())
    });
    assert!(result.unwrap_err().contains("100"));
    assert_eq!(doc, before);
    account::add_profile_item(&mut doc, 1100, 1).unwrap();
    assert!(
        crate::app::account_validation::validate_new_account_catalog_issues(
            &doc, &before, &catalog, false
        )
        .unwrap_err()
        .contains("100")
    );
}
