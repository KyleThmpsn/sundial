//! Helpers the tests share.
use std::{
    hash::{Hash, Hasher},
    path::Path,
};

pub(crate) mod driver;

use sundial::investment::{
    InvestmentCatalog, WeaponDamageProfile, WeaponDonorSummary, WeaponRarity,
};
use sundial::package_authoring::runtime::{
    BindingHash, SchemaHandle, WeaponRuntimeField, WeaponRuntimeFieldLocator,
    WeaponRuntimeFieldSource, WeaponRuntimeRootKind, WeaponRuntimeValue, WeaponRuntimeValueKind,
};

/// A generated-schema field named `name` at one fixed place in a weapon component's definition,
/// for the runtime value editors' tests.
pub(crate) fn runtime_field(
    name: &str,
    kind: WeaponRuntimeValueKind,
    value: WeaponRuntimeValue,
) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: BindingHash::new(0xB176_70ED),
            resource_index: 0,
            root: WeaponRuntimeRootKind::ComponentDefinition,
            root_schema: SchemaHandle::new(0x8080_388F),
            path: Vec::new(),
            type_handle: SchemaHandle::new(0x8080_2F16),
            value_offset: 0x48,
            byte_size: kind.byte_size(),
        },
        owner_offset: 0x100,
        name: name.into(),
        path_label: format!("Component / {name}"),
        kind,
        value,
        source: WeaponRuntimeFieldSource::GeneratedSchema,
        generated_kind: None,
        name_inferred: false,
    }
}

/// A synthetic donor: a collection-backed Legendary in bucket 0 with no known Power cap, slot,
/// damage, ammo, sandbox pattern or stat group. A test sets what it checks by struct update,
/// so each fixture shows only the fields that matter to it.
pub(crate) fn donor_summary(hash: u32, name: &str, type_name: &str) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash,
        name: name.into(),
        type_name: type_name.into(),
        bucket_hash: 0,
        collection_backed: true,
        power_cap: None,
        damage_type: None,
        inventory_slot: None,
        ammo_type: None,
        weapon_pattern_index: None,
        weapon_translation_group: None,
        stat_group_index: None,
        damage_profile: WeaponDamageProfile::Unknown,
        rarity: WeaponRarity::Legendary,
    }
}

/// Optional receipts from filesystem and worker workflows, alongside the UI captures.
pub(crate) fn artifact(name: &str, value: &impl serde::Serialize) {
    let Some(directory) = std::env::var_os("PARHELION_TEST_ARTIFACTS") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

/// Loads `install`'s catalog through a cache of the tests' own. A test never replaces the app's
/// shared cache, and each package set is scanned once rather than on every run. Sundial's tests
/// use the same folder.
pub(crate) fn catalog(install: &Path) -> Result<InvestmentCatalog, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    install.hash(&mut hasher);
    let cache = std::env::temp_dir()
        .join("sundial-test-catalogs")
        .join(format!("{:016x}.json", hasher.finish()));
    InvestmentCatalog::load_with_cache_path(install, &cache, false, |_| {})
}
