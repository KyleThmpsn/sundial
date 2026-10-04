//! Experimental, atomic weapon imports with their private runtime dependencies.
use std::{fs, path::Path};

use parhelion_import::d2_mot::{
    gameplay::perks::{lower, translate},
    payload::Payload,
    reader::{Reader, outside},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sundial::{
    investment::{InvestmentCatalog, WeaponDonor},
    package_authoring::{
        PackageManager,
        entity::{WEAPON_ENTITY_CLASS, weapon_component_bindings},
        open_shadowkeep_package_manager, resolve_live_named_tag,
        runtime::load_weapon_runtime_entity_with_manager,
        sandbox_perk::load_sandbox_perk_runtime_action,
        sandbox_perk::program::{
            Action, NativeAssetPatch, NativeAssetResourcePatch, NativeNode, Program, Trigger,
        },
    },
};
use tiger_pkg::TagHash;

use crate::{
    ItemKind,
    recipe::{
        HexHash, SwordProfileRecipe, WeaponRecipe, WeaponSocketColumnRecipe,
        WeaponSocketPlugVariantRecipe,
    },
    tag_payload::{array_at, read_array, read_u32, read_u64, relative_target},
};

const SWORD_BINDING: u32 = 0xCD2B_CEAC;
const SWORD_CLASS: u32 = 0x8080_43D2;
// This shipped effect graph supplies the native modifier envelope. Its resource row is
// discovered and checked, rather than identifying the imported behavior by an item hash.
const LUNGE_GRAPH: u32 = 0x8162_C91A;
const LUNGE_BINDING: u32 = 0x7330_E39F;

pub struct Request<'a> {
    pub modern_packages: &'a Path,
    pub native_packages: &'a Path,
    pub output: &'a Path,
    pub recipe: &'a WeaponRecipe,
    pub plug_hash: u32,
    pub socket_index: u16,
    pub choice_index: u16,
    pub source_plug_hash: u32,
    pub source_perk_index: u16,
    pub profile_key: u32,
    pub name: &'a str,
}

/// A candidate retains its full baseline so applying it cannot replace intervening edits.
/// Native package validation is still followed by separate event and gameplay verification.
pub struct Prepared {
    baseline: WeaponRecipe,
    recipe: WeaponRecipe,
    pub provenance: Value,
}

impl Prepared {
    pub fn recipe(&self) -> &WeaponRecipe {
        &self.recipe
    }

    pub fn apply(&self, recipe: &mut WeaponRecipe) -> Result<(), String> {
        if *recipe != self.baseline {
            return Err(
                "The weapon changed after import preparation. Prepare the import again.".into(),
            );
        }
        *recipe = self.recipe.clone();
        Ok(())
    }
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn checked_tag(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Native tag {tag:08X} is missing"))?;
    if entry.reference != class {
        return Err(format!(
            "Native tag {tag:08X} has class {:08X}, expected {class:08X}",
            entry.reference
        ));
    }
    let bytes = manager
        .read_tag(TagHash(tag))
        .map_err(|error| format!("Could not read native tag {tag:08X}: {error}"))?;
    if usize::try_from(entry.file_size).ok() != Some(bytes.len()) {
        return Err(format!(
            "Native tag {tag:08X} decoded size differs from its package entry"
        ));
    }
    Ok(bytes)
}

fn sword(manager: &PackageManager, hash: u32) -> Result<(), String> {
    let entity = load_weapon_runtime_entity_with_manager(manager, hash)?;
    let bindings = weapon_component_bindings(&entity.payload, SWORD_BINDING)?;
    if bindings.len() != 1 || bindings[0].concrete_class != SWORD_CLASS {
        return Err(format!(
            "Gameplay donor {hash:08X} does not carry a supported native sword component"
        ));
    }
    Ok(())
}

fn selection(
    request: &Request<'_>,
    catalog: &InvestmentCatalog,
    donor: &WeaponDonor,
) -> Result<Vec<HexHash>, String> {
    let index = usize::from(request.socket_index);
    let columns = &request.recipe.overrides.socket_columns;
    let socket = donor.sockets.get(index);
    let column = columns.get(index).and_then(Option::as_ref);
    if socket.is_none() && column.is_none() {
        return Err("The selected socket does not exist on this weapon".into());
    }
    let choices = if let Some(column) = column {
        column.choices.clone()
    } else {
        let socket = socket.ok_or("The selected socket has no native layout")?;
        if socket.max_authored_choices == 0 {
            return Err("The selected socket is disabled".into());
        }
        let mut choices = Vec::new();
        if let Some(default) = socket.native_default {
            choices.push(HexHash::new(default));
        }
        for &hash in &socket.ordered_embedded_choices {
            if choices.len() >= socket.max_authored_choices {
                break;
            }
            let hash = HexHash::new(hash);
            if !choices.contains(&hash) {
                choices.push(hash);
            }
        }
        choices
    };
    let selected = choices
        .get(usize::from(request.choice_index))
        .ok_or("The selected socket choice does not exist")?;
    if selected.parse_u32().map_err(error)? != request.source_plug_hash {
        return Err("The selected socket choice no longer matches the native source plug".into());
    }
    let types = columns
        .iter()
        .map(|column| column.as_ref().and_then(|column| column.socket_type))
        .collect::<Vec<_>>();
    let supported = catalog.supported_plug_sets(donor.summary.hash, &types)?;
    if !supported
        .get(index)
        .is_some_and(|set| set.plug_hashes.contains(&request.source_plug_hash))
    {
        return Err("The native source plug is incompatible with the selected socket".into());
    }
    if !catalog
        .item_sandbox_perk_indices(request.source_plug_hash)
        .contains(&request.source_perk_index)
    {
        return Err("The selected finished perk does not belong to the native source plug".into());
    }
    Ok(choices)
}

struct Lunge {
    patch: NativeAssetPatch,
    owner_tag: u32,
    offset: usize,
    resource_offset: usize,
    graph: Vec<u8>,
    owner: Vec<u8>,
}

fn lunge(manager: &PackageManager, source: &translate::Source) -> Result<Lunge, String> {
    let graph = checked_tag(manager, LUNGE_GRAPH, WEAPON_ENTITY_CLASS)?;
    let bindings = weapon_component_bindings(&graph, LUNGE_BINDING)?;
    if bindings.len() != 1
        || bindings[0].resource_count != 1
        || bindings[0].concrete_class != 0x8080_3B05
    {
        return Err("The native lunge modifier binding is ambiguous".into());
    }
    let binding = bindings[0];
    let owner_class = manager
        .get_entry(TagHash(binding.owner_tag))
        .ok_or("The native lunge owner is missing")?
        .reference;
    let owner = checked_tag(manager, binding.owner_tag, owner_class)?;
    let resource = usize::try_from(binding.resource_offset).map_err(error)?;
    // The graph selects a modifier resource. The instance and settings roots
    // carry their own classes separately from that resource descriptor.
    let instance = relative_target(&owner, 16).map_err(error)?;
    let settings = relative_target(&owner, 24).map_err(error)?;
    if instance < 4
        || settings < 4
        || read_u32(&owner, instance - 4).map_err(error)? != 0x8080_3AFD
        || read_u32(&owner, instance + 4).map_err(error)? != 0x8080_3AFE
        || read_u32(&owner, settings - 4).map_err(error)? != 0x8080_3AFE
        || read_u32(&owner, settings + 4).map_err(error)? != 0x8080_3AFD
    {
        return Err("The native lunge instance and settings classes are unsupported".into());
    }
    let descriptor = settings
        .checked_add(88)
        .ok_or("The native modifier descriptor overflows")?;
    let (count, _, rows, class) = array_at(&owner, descriptor).map_err(error)?;
    if class != 0x8080_3B06 || count == 0 || count > 256 {
        return Err("The native modifier settings array is unsupported".into());
    }
    let mut candidates = Vec::new();
    for index in 0..count {
        let at = rows
            .checked_add(
                index
                    .checked_mul(88)
                    .ok_or("Native modifier size overflows")?,
            )
            .ok_or("Native modifier offset overflows")?;
        read_array::<88>(&owner, at).map_err(error)?;
        // The shipped envelope can target another component. Its graph resource and
        // reciprocal runtime/settings pair identify the row to translate.
        if read_u32(&owner, at).map_err(error)? == binding.owner_tag
            && read_u32(&owner, at + 4).map_err(error)? == binding.concrete_class
            && read_u64(&owner, at + 8).map_err(error)? == resource as u64
            && read_u64(&owner, resource + 8).map_err(error)? == at as u64
        {
            candidates.push(at);
        }
    }
    if candidates.len() != 1 {
        return Err("The native lunge resource requires exactly one paired modifier row".into());
    }
    let offset = candidates[0];
    if read_u32(&owner, resource).map_err(error)? != binding.owner_tag
        || read_u32(&owner, resource + 4).map_err(error)? != class
        || read_u64(&owner, resource + 8).map_err(error)? != offset as u64
        || read_u64(&owner, offset + 8).map_err(error)? != resource as u64
    {
        return Err("The selected native lunge resource and modifier row do not match".into());
    }
    let relative = offset
        .checked_sub(resource)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or("The native lunge modifier lies outside its component resource")?;
    let expected = read_array::<88>(&owner, offset).map_err(error)?.to_vec();
    let bytes = lower::modifier_settings(
        &source.lunge.owner,
        source.lunge.modifier_offset,
        &Payload(owner.clone()),
        offset,
    )
    .map_err(error)?
    .to_vec();
    Ok(Lunge {
        patch: NativeAssetPatch {
            action_index: 0,
            source_graph: LUNGE_GRAPH,
            appends: Vec::new(),
            remove_owners: Vec::new(),
            patches: vec![NativeAssetResourcePatch {
                binding_hash: LUNGE_BINDING,
                resource_index: u16::try_from(binding.resource_index).map_err(error)?,
                offset: relative,
                expected,
                bytes,
                imported_particle: None,
            }],
        },
        owner_tag: binding.owner_tag,
        offset,
        resource_offset: resource,
        graph,
        owner,
    })
}

fn node(node: lower::Node) -> NativeNode {
    NativeNode {
        kind: node.kind,
        bytes: node.bytes,
    }
}

fn program(
    request: &Request<'_>,
    source: &translate::Source,
    patch: NativeAssetPatch,
) -> Result<Program, String> {
    let mut draw = NativeNode::condition(16).ok_or("The native draw condition is unavailable")?;
    if draw.bytes.len() < 112 {
        return Err("The native draw condition layout differs".into());
    }
    let prefix = lower::draw_condition_prefix(&source.controller, source.routing.draw_offset)
        .map_err(error)?;
    draw.bytes[..prefix.len()].copy_from_slice(&prefix);
    let mut removals = source
        .routing
        .removals
        .iter()
        .map(|&offset| {
            lower::condition(&source.controller, offset)
                .map(node)
                .map_err(error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if removals.iter().map(|node| node.kind).collect::<Vec<_>>() != [18, 1, 41, 8, 20] {
        return Err("The source removal cycle cannot be represented by this native adapter".into());
    }
    let timer =
        lower::timer_condition(&source.controller, source.routing.rearm_offset).map_err(error)?;
    let mut expected_timer = vec![0; 12];
    expected_timer[..4].copy_from_slice(&1.0f32.to_le_bytes());
    expected_timer[4] = 0xFF;
    expected_timer[5] = 1;
    expected_timer[7] = timer.bytes[7]; // Ordinals are rebuilt by the compiler.
    expected_timer[8..12]
        .copy_from_slice(&(source.routing.cooldown_ms as f32 / 1000.0).to_le_bytes());
    if timer.bytes != expected_timer || source.routing.cooldown_ms == 0 {
        return Err("The source rearm timer cannot be represented by the native cooldown".into());
    }
    let lunge = lower::dynamic_entity(&source.controller, source.lunge.effect_offset, LUNGE_GRAPH)
        .map(node)
        .map_err(error)?;
    let movement = lower::host_record(&source.controller, source.movement_offset)
        .map(node)
        .map_err(error)?;
    let mut key =
        NativeNode::effect(41).ok_or("The native sword profile key effect is unavailable")?;
    if key.bytes.len() < 8 {
        return Err("The native sword profile key effect layout differs".into());
    }
    key.bytes[1] = 1;
    key.bytes[4..8].copy_from_slice(&request.profile_key.to_le_bytes());
    let program = Program {
        name: request.name.into(),
        trigger: Trigger::Native,
        duration_ms: 0,
        cooldown_ms: source.routing.cooldown_ms,
        native_trigger: Some(draw),
        native_removal: Some(removals.remove(0)),
        alternative_removals: removals,
        actions: vec![
            Action::Native { node: lunge },
            Action::Native { node: key },
            Action::Native { node: movement },
        ],
        native_asset_patches: vec![patch],
        ..Program::default()
    };
    program.validate()?;
    Ok(program)
}

/// Prepare one source-validated controller as an experimental private native candidate.
/// Source auxiliary consumers and native swing publication remain explicit gaps.
pub fn prepare(request: &Request<'_>) -> Result<Prepared, String> {
    if request.recipe.kind != ItemKind::Weapon {
        return Err("A sword perk import requires a weapon recipe".into());
    }
    if request
        .recipe
        .overrides
        .socket_plug_variants
        .iter()
        .any(|variant| {
            variant.socket_index == request.socket_index
                && variant.choice_index == request.choice_index
        })
    {
        return Err("The selected socket choice already has a private perk".into());
    }
    request
        .recipe
        .to_spec()
        .map_err(error)?
        .validate()
        .map_err(error)?;
    // Resolve both package trees before creating the isolated catalog and evidence files.
    let modern = request.modern_packages.canonicalize().map_err(error)?;
    let native = request.native_packages.canonicalize().map_err(error)?;
    let output = outside(request.output, modern.parent().unwrap_or(&modern)).map_err(error)?;
    outside(&output, native.parent().unwrap_or(&native)).map_err(error)?;
    fs::create_dir_all(&output).map_err(error)?;
    let manager = open_shadowkeep_package_manager(&native)?;
    if manager.package_dir.canonicalize().map_err(error)? != native {
        return Err(
            "The native package reader did not open the requested package directory".into(),
        );
    }
    let donor_hash = request.recipe.donor.item_hash.parse_u32().map_err(error)?;
    sword(&manager, donor_hash)?;
    if let Some(hash) = &request.recipe.overrides.weapon_pattern_donor_hash {
        sword(&manager, hash.parse_u32().map_err(error)?)?;
    }
    for component in &request.recipe.runtime_component_donors {
        if component.binding_hash.parse_u32().map_err(error)? == SWORD_BINDING {
            sword(
                &manager,
                component.donor.item_hash.parse_u32().map_err(error)?,
            )?;
        }
    }
    let catalog = InvestmentCatalog::load_with_cache_path(
        native
            .parent()
            .ok_or("The native packages have no installation parent")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )?;
    let donor = catalog
        .weapon_donor(donor_hash)
        .ok_or("The gameplay donor is absent from the native catalog")?;
    let choices = selection(request, &catalog, &donor)?;
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals = manager.read_tag(globals_tag)?;
    load_sandbox_perk_runtime_action(&manager, &globals, usize::from(request.source_perk_index))
        .map_err(|error| {
            format!("The selected native finished perk has no usable runtime: {error}")
        })?;
    let mut reader = Reader::new(&modern, &output.join("source"), true).map_err(error)?;
    let source = translate::extract(&mut reader, request.plug_hash).map_err(error)?;
    let profile = SwordProfileRecipe {
        key: HexHash::new(request.profile_key),
        near_scale_bits: source.angular.scales.near_bits,
        far_scale_bits: source.angular.scales.far_bits,
    };
    if request
        .recipe
        .overrides
        .sword_profile
        .as_ref()
        .is_some_and(|existing| *existing != profile)
    {
        return Err("The weapon already carries an incompatible private sword profile".into());
    }
    let native_lunge = lunge(&manager, &source)?;
    let program = program(request, &source, native_lunge.patch.clone())?;
    let mut recipe = request.recipe.clone();
    let index = usize::from(request.socket_index);
    if recipe
        .overrides
        .socket_columns
        .get(index)
        .and_then(Option::as_ref)
        .is_none()
    {
        let count = donor
            .sockets
            .len()
            .max(recipe.overrides.socket_columns.len())
            .max(index + 1);
        recipe.overrides.socket_columns.resize_with(count, || None);
        recipe.overrides.socket_columns[index] = Some(WeaponSocketColumnRecipe {
            choices,
            ..WeaponSocketColumnRecipe::default()
        });
    }
    let mut effect = super::PerkRecipe::effect(request.source_perk_index);
    effect.program = Some(program);
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
        replace_effects: true,
        socket_index: request.socket_index,
        choice_index: request.choice_index,
        source_plug_hash: HexHash::new(request.source_plug_hash),
        name: Some(request.name.into()),
        description: Some(
            "Experimental source-derived sword perk candidate. Gameplay verification is pending."
                .into(),
        ),
        icon: None,
        classification_donor_hash: None,
        investment_stats: Vec::new(),
        additional_sandbox_perks: Vec::new(),
        sandbox_perks: vec![effect],
    });
    recipe.overrides.sword_profile = Some(profile);
    recipe.to_spec().map_err(error)?.validate().map_err(error)?;
    reader.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({
        "status": "experimental package candidate",
        "full_perk_installable": false,
        "gameplay_verified": false,
        "prepared_recipe_sha256": digest(encoded.as_bytes()),
        "baseline_recipe_sha256": digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
        "remaining": ["Unknown source auxiliary consumer", "Native swing publication and channel-2 timing proof", "Gameplay verification"],
        "source": {
            "plug_hash": source.plug_hash,
            "item_tag": source.item_tag,
            "perk_hash": source.perk_hash,
            "runtime_key": source.runtime_key,
            "action_tag": source.action_tag,
            "controller_sha256": digest(&source.controller.0),
            "routing": source.routing,
            "lunge_entity": source.lunge.entity_tag,
            "lunge_owner": source.lunge.owner_tag,
            "lunge_modifier_offset": source.lunge.modifier_offset,
            "lunge_owner_sha256": digest(&source.lunge.owner.0),
            "angular_entity": source.angular.entity_tag,
            "angular_owner": source.angular.owner_tag,
            "angular_owner_sha256": digest(&source.angular.owner.0),
        },
        "native": {
            "donor_hash": donor_hash,
            "socket_index": request.socket_index,
            "choice_index": request.choice_index,
            "source_plug_hash": request.source_plug_hash,
            "source_perk_index": request.source_perk_index,
            "lunge_graph": LUNGE_GRAPH,
            "lunge_owner": native_lunge.owner_tag,
            "lunge_resource_offset": native_lunge.resource_offset,
            "lunge_modifier_offset": native_lunge.offset,
            "lunge_graph_sha256": digest(&native_lunge.graph),
            "lunge_owner_sha256": digest(&native_lunge.owner),
            "profile": recipe.overrides.sword_profile,
            "effect_order": [2, 41, 36],
            "removal_order": [18, 1, 41, 8, 20],
        }
    });
    fs::write(output.join("native-lunge-graph.bin"), &native_lunge.graph).map_err(error)?;
    fs::write(output.join("native-lunge-owner.bin"), &native_lunge.owner).map_err(error)?;
    fs::write(output.join("prepared-weapon.parhelion.json"), encoded).map_err(error)?;
    fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance).map_err(error)?,
    )
    .map_err(error)?;
    Ok(Prepared {
        baseline: request.recipe.clone(),
        recipe,
        provenance,
    })
}
