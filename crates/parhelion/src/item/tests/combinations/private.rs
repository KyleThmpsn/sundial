use super::*;
use sundial::package_authoring::{
    runtime::resolve_weapon_runtime_field, sandbox_perk::sandbox_perk_action_boxed_value_offset,
};

#[derive(Default)]
pub(super) struct PrivateIdentities {
    /// Each private plug and the variant that produced it. Identical definitions share one
    /// plug across weapons, so a repeated hash is a collision only when the definitions differ.
    pub(super) plugs: BTreeMap<u32, crate::item::WeaponSocketPlugVariantOverride>,
    perks: BTreeMap<u16, (u32, u16)>,
    actions: BTreeMap<TagHash, (u32, u16)>,
    values: usize,
}

fn verify_values(
    manager: &sundial::package_authoring::PackageManager,
    action: &sundial::package_authoring::sandbox_perk::SandboxPerkRuntimeAction,
    source_action: &sundial::package_authoring::sandbox_perk::SandboxPerkRuntimeAction,
    edit: &WeaponSandboxPerkRuntimeOverride,
) -> usize {
    for value in &edit.runtime_values {
        let matching = action
            .graphs
            .iter()
            .filter_map(|graph| {
                let mut locator = value.locator.clone();
                if let Some(source_tag) = locator.graph_tag.map(|tag| tag.get()) {
                    let source = source_action
                        .graphs
                        .iter()
                        .find(|source| source.tag.0 == source_tag)?;
                    if !graph
                        .action_offsets
                        .iter()
                        .any(|offset| source.action_offsets.contains(offset))
                    {
                        return None;
                    }
                    locator.graph_tag = None;
                }
                resolve_weapon_runtime_field(manager, &graph.payload, &locator).ok()
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].field.value, value.value);
    }
    for value in &edit.action_float_values {
        let offset = sandbox_perk_action_boxed_value_offset(
            &action.action_payload,
            value.node_type_handle,
            value.node_occurrence,
            value.value_pointer_offset,
            value.value_type_handle,
            4,
        )
        .unwrap();
        assert_eq!(
            read_u32(&action.action_payload, offset).unwrap(),
            value.value_bits
        );
    }
    edit.runtime_values.len() + edit.action_float_values.len()
}

pub(super) fn verify_private(
    manager: &sundial::package_authoring::PackageManager,
    tables: &Tables,
    source_manager: &sundial::package_authoring::PackageManager,
    source_globals: &[u8],
    recipe: &WeaponRecipe,
    identities: &mut PrivateIdentities,
) {
    let spec = recipe.to_spec().unwrap();
    let (definition, _) = tables.load(manager, spec.identity.item_hash);
    let resource = relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, sockets, _) = array_at(&definition, resource).unwrap();
    for variant in &spec.overrides.socket_plug_variants {
        let row = sockets + usize::from(variant.socket_index) * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        let (_, _, choices, _) = array_at(
            &definition,
            row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
        )
        .unwrap();
        let plug_index = read_u16(
            &definition,
            choices + usize::from(variant.choice_index) * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE,
        )
        .unwrap();
        let plug_hash = tables.plug_hash(plug_index);
        assert_ne!(plug_hash, variant.source_plug_hash);
        if let Some(previous) = identities.plugs.insert(plug_hash, variant.clone()) {
            assert!(
                previous.same_definition(variant),
                "private plugs collided across weapons with different definitions"
            );
        }
        let (plug, _) = tables.load(manager, plug_hash);
        let (source, _) = tables.load(manager, variant.source_plug_hash);
        let perks = weapon_sandbox_perks(&plug).unwrap();
        let stock_perks = weapon_sandbox_perks(&source).unwrap();
        assert_eq!(
            perks.len(),
            stock_perks.len() + variant.additional_sandbox_perks.len()
        );
        verify_additional_perks(
            manager,
            &tables.globals,
            &perks[stock_perks.len()..],
            &variant.additional_sandbox_perks,
        );
        for edit in &variant.sandbox_perks {
            let position = stock_perks
                .iter()
                .position(|index| *index == edit.source_perk_index)
                .unwrap();
            let private_index = perks[position];
            assert_ne!(private_index, edit.source_perk_index);
            let owner = (plug_hash, edit.source_perk_index);
            if let Some(previous) = identities.perks.insert(private_index, owner) {
                assert_eq!(previous, owner, "different private finished perks collided");
            }
            let action = load_sandbox_perk_runtime_action(
                manager,
                &tables.globals,
                usize::from(private_index),
            )
            .unwrap();
            let source_action = load_sandbox_perk_runtime_action(
                source_manager,
                source_globals,
                usize::from(edit.source_perk_index),
            )
            .unwrap();
            let unchanged = edit.program.is_none()
                && edit.projectiles.is_empty()
                && edit.runtime_values.is_empty()
                && edit.action_float_values.is_empty()
                && edit.activation.is_none();
            verify_action_identity(
                identities,
                owner,
                unchanged,
                action.action_tag,
                source_action.action_tag,
            );
            identities.values += verify_values(manager, &action, &source_action, edit);
            assert_eq!(
                manager.read_tag(source_action.action_tag).unwrap(),
                source_action.action_payload
            );
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT"]
fn native_bundled_variants_keep_private_identities_separate() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("private-catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let mut recipes = Vec::new();
    for copy in 0..3 {
        for (_, encoded) in crate::recipe_library::BUNDLED_RECIPES {
            let mut recipe = WeaponRecipe::from_json_str(encoded).unwrap();
            recipe
                .rename_authored_item(format!("Private Batch {copy} {}", recipe.name))
                .unwrap();
            recipe.overrides.ammo_type = Some(
                [
                    RecipeAmmoType::Primary,
                    RecipeAmmoType::Special,
                    RecipeAmmoType::Heavy,
                ][copy],
            );
            // Keep each prototype's supported placement and rarity while changing ammo and identity.
            let donor = catalog
                .weapon_donor(recipe.donor.item_hash.parse_u32().unwrap())
                .unwrap();
            recipe.overrides.inventory_slot.get_or_insert_with(|| {
                match donor.summary.inventory_slot.unwrap() {
                    sundial::investment::WeaponInventorySlot::Kinetic => {
                        RecipeInventorySlot::Kinetic
                    }
                    sundial::investment::WeaponInventorySlot::Energy => RecipeInventorySlot::Energy,
                    sundial::investment::WeaponInventorySlot::Power => RecipeInventorySlot::Power,
                }
            });
            // A variable-damage weapon rests on the first of its own elements, never Kinetic.
            if recipe.overrides.variable_damage.is_none() {
                recipe
                    .overrides
                    .modern_damage_type
                    .get_or_insert(RecipeDamageType::Kinetic);
            }
            recipe.overrides.rarity.get_or_insert(RecipeRarity::Exotic);
            for variant in &mut recipe.overrides.socket_plug_variants {
                for perk in &mut variant.sandbox_perks {
                    for value in &mut perk.action_float_values {
                        value.value_bits =
                            (f32::from_bits(value.value_bits) * [0.5, 1.0, 2.0][copy]).to_bits();
                    }
                }
            }
            recipe.validate().unwrap();
            recipes.push(recipe);
        }
    }
    let source_manager = open_manager(&packages).unwrap();
    let source_globals = Tables::read(&source_manager).globals;
    let mut identities = PrivateIdentities::default();
    let (build, ammo) = run_batch(
        &packages,
        &output,
        "private-batch",
        &recipes,
        |manager, tables, recipe| {
            let ammo = verify_fields(manager, tables, recipe);
            verify_private(
                manager,
                tables,
                &source_manager,
                &source_globals,
                recipe,
                &mut identities,
            );
            ammo
        },
    );
    assert!(!identities.plugs.is_empty());
    assert!(!identities.actions.is_empty());
    assert!(identities.values > 0);
    eprintln!(
        "PRIVATE_BATCH_PASS weapons={} plugs={} perks={} actions={} runtime_values={} ammo_variants={ammo} stage={}",
        recipes.len(),
        identities.plugs.len(),
        identities.perks.len(),
        identities.actions.len(),
        identities.values,
        build.run_directory.display()
    );
}

fn verify_action_identity(
    identities: &mut PrivateIdentities,
    owner: (u32, u16),
    unchanged: bool,
    authored: TagHash,
    stock: TagHash,
) {
    if unchanged {
        assert_eq!(authored, stock);
    } else {
        assert_ne!(authored, stock);
        if let Some(previous) = identities.actions.insert(authored, owner) {
            assert_eq!(
                previous, owner,
                "different private mutable actions collided"
            );
        }
        assert_eq!(authored.pkg_id(), PRIVATE_PERK_RUNTIME_PACKAGE_ID);
    }
}
