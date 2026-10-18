//! Prepare a plug whose controllers last for the weapon's lifetime, from equip to unequip or
//! for as long as the perk is applied, and write named properties such as a rocket channel.
use super::*;
use parhelion_import::d2_mot::gameplay::perks::{Assignment, lifecycle};

/// The prepared candidate, or `None` when no controller of the plug has this shape.
pub(super) fn prepare(
    request: &Request<'_>,
    context: (&PackageManager, &InvestmentCatalog),
    reader: &mut Reader,
    output: &Path,
    donor: &WeaponDonor,
    assigned: &[Assignment],
    choices: Vec<HexHash>,
) -> Result<Option<Prepared>, String> {
    let (manager, catalog) = context;
    let mut translation = lifecycle::translate(assigned, request.name);
    if translation.programs.is_empty() {
        return Ok(None);
    }
    let presentation = presentation::read(reader, request.plug_hash).map_err(error)?;
    let name = if request.name.trim().is_empty() {
        presentation.name.clone()
    } else {
        request.name.into()
    };
    for program in &mut translation.programs {
        program.name.clone_from(&name);
    }
    // The selected finished perk carries the first program. Further programs replace the
    // runtimes of the stock plug's other finished perks, one each, since no stock plug repeats
    // a finished perk.
    let mut templates = vec![request.source_perk_index];
    if translation.programs.len() > 1 {
        let tag = catalog
            .item_definition_tag(request.source_plug_hash)
            .ok_or("The selected stock plug has no item definition")?;
        let plug = checked_tag(manager, tag, 0x8080_7BEA)?;
        for index in sundial::package_authoring::native_weapon::base_sandbox_perks(&plug)? {
            if !templates.contains(&index) {
                templates.push(index);
            }
        }
    }
    let mut omitted = translation.omitted;
    let mut sandbox_perks = Vec::new();
    for (position, program) in translation.programs.into_iter().enumerate() {
        let Some(&index) = templates.get(position) else {
            omitted.push(json!({"program":program.name,
                "reason":"the selected plug has no further finished perk to carry this program"}));
            continue;
        };
        program.validate()?;
        let mut effect = super::super::PerkRecipe::effect(index);
        effect.program = Some(program);
        sandbox_perks.push(effect);
    }
    let mut recipe = request.recipe.clone();
    let socket = usize::from(request.socket_index);
    if recipe
        .overrides
        .socket_columns
        .get(socket)
        .and_then(Option::as_ref)
        .is_none()
    {
        let count = donor
            .sockets
            .len()
            .max(recipe.overrides.socket_columns.len())
            .max(socket + 1);
        recipe.overrides.socket_columns.resize_with(count, || None);
        recipe.overrides.socket_columns[socket] = Some(WeaponSocketColumnRecipe {
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
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
            projectile: sandbox_perks
                .iter()
                .filter_map(|effect| effect.program.as_ref())
                .find_map(|program| program.projectile_dependency()),
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
            sandbox_perks,
        });
    recipe.to_spec().map_err(error)?.validate().map_err(error)?;
    reader.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({
        "status":"experimental lifecycle property candidate", "gameplay_verified":false,
        "presentation":presentation.report,
        "prepared_recipe_sha256":digest(encoded.as_bytes()),
        "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
        "source":{"plug_hash":request.plug_hash,"controllers":assigned.iter().map(|a| json!({
            "action_tag":format!("{:08X}", a.action_tag),"perk_hash":format!("{:08X}", a.perk_hash),
            "controller_sha256":digest(&a.controller.0)})).collect::<Vec<_>>()},
        "omitted":omitted,
        "remaining":["In-game equip and unequip, the native consumer of each written property", "Stock donor gameplay isolation"]
    });
    fs::write(output.join("prepared-weapon.parhelion.json"), encoded).map_err(error)?;
    fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance).map_err(error)?,
    )
    .map_err(error)?;
    Ok(Some(Prepared {
        baseline: request.recipe.clone(),
        recipe,
        provenance,
    }))
}
