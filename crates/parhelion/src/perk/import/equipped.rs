//! Prepare a source-validated equip lifecycle using a privately cloned modifier graph.
use super::*;
use parhelion_import::d2_mot::gameplay::perks::equipped;
use sundial::package_authoring::sandbox_perk::program::{Asset, AttachmentTarget};

pub(super) fn prepare(
    request: &Request<'_>,
    manager: &PackageManager,
    reader: &mut Reader,
    output: &Path,
    donor: &WeaponDonor,
    choices: Vec<HexHash>,
) -> Result<Prepared, String> {
    let source = equipped::extract(reader, request.plug_hash).map_err(error)?;
    let presentation = presentation::read(reader, request.plug_hash).map_err(error)?;
    let mut native = modifier(manager, &source.owner, source.modifier_offset)?;
    // The stock envelope also contains a presentation owner. The source constant
    // modifier has no such component, so leave it out of this private copy.
    let graph = Payload(native.graph.clone());
    for row in graph.array(16, 12, Some(0x80809C04)).map_err(error)? {
        let tag = graph.u32(row).map_err(error)?;
        if tag != native.owner_tag {
            let owner = Payload(checked_tag(manager, tag, 0x80809C36)?);
            let settings = owner.pointer(24).map_err(error)?;
            if owner.u32(settings - 4).map_err(error)? != 0x80804205 {
                return Err(
                    "The native modifier envelope has an unsupported extra component".into(),
                );
            }
            native.patch.remove_owners.push(tag);
        }
    }
    let name = if request.name.trim().is_empty() {
        presentation.name.clone()
    } else {
        request.name.into()
    };
    let program = Program {
        name: name.clone(),
        trigger: Trigger::Native,
        duration_ms: 0,
        cooldown_ms: 0,
        native_trigger: Some(node(source.activation)),
        native_removal: Some(node(source.removal)),
        actions: vec![Action::Attach {
            asset: Asset {
                graph: MODIFIER_GRAPH,
                ..Default::default()
            },
            mode: AttachmentTarget::ThisItem,
            keys: [0x811C9DC5; 2],
            float_bits: [0; 4],
        }],
        native_asset_patches: vec![native.patch],
        ..Program::default()
    };
    program.validate()?;
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
            ..Default::default()
        });
    }
    let icon = presentation
        .icon_png
        .as_deref()
        .map(|bytes| {
            crate::icon_edit::ImportedIcon::from_bytes(bytes).map(|image| {
                super::super::Icon::Image {
                    name: presentation.name.clone(),
                    image,
                }
            })
        })
        .transpose()?;
    let mut effect = super::super::PerkRecipe::effect(request.source_perk_index);
    effect.program = Some(program);
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
            projectile: None,
            offer_everywhere: false,
            replace_effects: true,
            socket_index: request.socket_index,
            choice_index: request.choice_index,
            source_plug_hash: HexHash::new(request.source_plug_hash),
            name: Some(name),
            description: Some(presentation.description.clone()),
            icon,
            classification_donor_hash: None,
            investment_stats: Vec::new(),
            additional_sandbox_perks: Vec::new(),
            sandbox_perks: vec![effect],
        });
    recipe.to_spec().map_err(error)?.validate().map_err(error)?;
    reader.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let at = source.modifier_offset;
    let provenance = json!({
        "status":"experimental equipped modifier candidate", "gameplay_verified":false,
        "presentation":presentation.report,
        "prepared_recipe_sha256":digest(encoded.as_bytes()),
        "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
        "source": {"plug_hash":request.plug_hash,"item_tag":source.assignment.item_tag,
            "action_tag":source.assignment.action_tag,"controller_sha256":digest(&source.assignment.controller.0),
            "entity":source.entity_tag,"owner":source.owner_tag,"path":source.path,
            "owner_sha256":digest(&source.owner.0),"modifier_offset":at,
            "amount":source.owner.f32(at + 16).map_err(error)?,
            "operation":source.owner.u8(at + 20).map_err(error)?,
            "component":source.owner.u8(at + 96).map_err(error)?,
            "input":source.owner.u16(at + 90).map_err(error)?},
        "native":{"template_graph":MODIFIER_GRAPH,"template_owner":native.owner_tag,
            "template_owner_sha256":digest(&native.owner),"activation":14,"removal":15,
            "attachment_target":0,"private_graph":true},
        "remaining":["In-game equip, repeated reload, perk selection and unequip cleanup", "Stock donor gameplay isolation"]
    });
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
