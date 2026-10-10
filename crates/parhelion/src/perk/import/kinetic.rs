//! A private, inspectable sustained-hit compatibility candidate.
use super::*;
use parhelion_import::d2_mot::{gameplay::perks::kinetic as source, native::attachment};
use std::collections::BTreeMap;
use sundial::package_authoring::{
    ability_damage,
    sandbox_perk::program::{Asset, ImportedAsset, Position},
};

mod pulse;

const ROOT: u32 = 0x80BC57DD;

fn write(bytes: &mut [u8], at: usize, value: &[u8]) -> Result<(), String> {
    bytes
        .get_mut(at..at.checked_add(value.len()).ok_or("pulse offset overflow")?)
        .ok_or("pulse write outside payload")?
        .copy_from_slice(value);
    Ok(())
}

fn array(
    bytes: &mut Vec<u8>,
    at: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<(), String> {
    if !rows.len().is_multiple_of(stride) {
        return Err("Counter array row width differs".into());
    }
    let count = (rows.len() / stride) as u64;
    let header = (bytes.len() + 4).next_multiple_of(16);
    bytes.resize(header, 0);
    write(bytes, header - 4, &0x80809FBDu32.to_le_bytes())?;
    bytes.extend(count.to_le_bytes());
    bytes.extend(u64::from(class).to_le_bytes());
    bytes.extend(rows);
    write(bytes, at, &count.to_le_bytes())?;
    write(
        bytes,
        at + 8,
        &(header as i64 - (at + 8) as i64).to_le_bytes(),
    )
}

fn counter(source: &source::Source) -> Result<NativeNode, String> {
    let mut bytes = vec![0; 208];
    write(&mut bytes, 0, &1f32.to_le_bytes())?;
    bytes[4] = 255;
    bytes[5] = 4;
    // The native evaluator must retain the per-target counter between events.
    bytes[6] = 1;
    bytes[0x80] = 255;
    bytes[0x91] = 1;
    bytes[0x99] = 1;
    bytes[0xA4] = 1;
    write(&mut bytes, 0x78, &1u32.to_le_bytes())?;
    write(&mut bytes, 0x9C, &0x811C9DC5u32.to_le_bytes())?;
    write(&mut bytes, 0x70, &0x80C70CA1u32.to_le_bytes())?;
    array(&mut bytes, 8, 0x80806B02, &0u32.to_le_bytes(), 4)?;
    let mut splash = [0u8; 24];
    splash[..4].copy_from_slice(&0x0989FD18u32.to_le_bytes());
    array(&mut bytes, 0x48, 0x808094B3, &splash, 24)?;
    let child = (bytes.len() + 4).next_multiple_of(16);
    bytes.resize(child, 0);
    write(&mut bytes, child - 4, &0x80803DF0u32.to_le_bytes())?;
    bytes.extend(source.hits.to_le_bytes());
    bytes.extend(source.hits.to_le_bytes());
    bytes.extend(source.timeout.to_le_bytes());
    write(&mut bytes, 0xC8, &(child as i64 - 0xC8).to_le_bytes())?;
    Ok(NativeNode { kind: 4, bytes })
}

fn node(
    directory: &Path,
    nodes: &mut Vec<Value>,
    symbol: &str,
    template: u32,
    bytes: &[u8],
    patches: Vec<Value>,
) -> Result<(), String> {
    let file = format!("{symbol}.bin");
    fs::write(directory.join(&file), bytes).map_err(error)?;
    nodes.push(json!({"symbol":symbol,"template":template,"file":file,"patches":patches}));
    Ok(())
}

pub(super) fn prepare(
    request: &Request<'_>,
    manager: &PackageManager,
    reader: &mut Reader,
    output: &Path,
    donor: &WeaponDonor,
    choices: Vec<HexHash>,
) -> Result<Prepared, String> {
    let source =
        source::extract(reader, request.plug_hash, &donor.summary.type_name).map_err(error)?;
    let presentation = presentation::read(reader, request.plug_hash).map_err(error)?;
    let directory = output.join("attachments");
    fs::create_dir_all(&directory).map_err(error)?;
    let mut native =
        Reader::new(request.native_packages, &output.join("native"), false).map_err(error)?;
    let namespace = format!(
        "{}/perk/{:08X}/{}/{}",
        request.recipe.namespace, request.plug_hash, request.socket_index, request.choice_index
    );
    let mut manifest = attachment::convert(
        reader,
        &mut native,
        &attachment::Request {
            source_tag: source.visual,
            source_packages: request.modern_packages,
            native_packages: request.native_packages,
            directory: &directory,
            namespace: &namespace,
        },
    )
    .map_err(error)?;
    let root = pulse::prepare(manager, &directory, &mut manifest, &source)?;
    let prefix = format!(
        "{:08X}-{:08X}-{}-{}",
        request.profile_key, request.plug_hash, request.socket_index, request.choice_index
    );
    let rename = |name: &str| format!("{prefix}-{name}");
    for section in ["particles", "attachments"] {
        for node in manifest[section]["nodes"]
            .as_array_mut()
            .ok_or("imported nodes")?
        {
            node["symbol"] = json!(rename(node["symbol"].as_str().ok_or("node symbol")?));
            if let Some(reference) = node["reference"].as_str() {
                node["reference"] = json!(rename(reference));
            }
            for patch in node["patches"].as_array_mut().ok_or("node patches")? {
                patch["symbol"] = json!(rename(patch["symbol"].as_str().ok_or("patch symbol")?));
            }
        }
    }
    for symbol in manifest["attachments"]["roots"]
        .as_array_mut()
        .ok_or("attachment roots")?
    {
        *symbol = json!(rename(symbol.as_str().ok_or("root symbol")?));
    }
    for symbol in manifest["particles"]["systems"]
        .as_object_mut()
        .ok_or("particle systems")?
        .values_mut()
    {
        *symbol = json!(rename(symbol.as_str().ok_or("particle symbol")?));
    }
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&manifest).map_err(error)?,
    )
    .map_err(error)?;
    let sha256 = attachment::fingerprint(&directory).map_err(error)?;
    let name = if request.name.trim().is_empty() {
        presentation.name.clone()
    } else {
        request.name.into()
    };
    let program = Program {
        name: name.clone(),
        trigger: Trigger::Native,
        native_trigger: Some(counter(&source)?),
        duration_ms: 0,
        cooldown_ms: source.cooldown_ms,
        actions: vec![Action::Spawn {
            asset: Asset {
                graph: ROOT,
                ..Default::default()
            },
            position: Position::Event,
        }],
        imported_assets: vec![ImportedAsset {
            action_index: 0,
            directory,
            sha256,
            symbol: rename(&root),
        }],
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
    let mut effect = super::super::PerkRecipe::effect(request.source_perk_index);
    effect.program = Some(program);
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
    native.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({"status":"experimental compatibility candidate","gameplay_verified":false,
        "full_perk_installable":false,"source_plug":request.plug_hash,"source_action":source.assignment.action_tag,
        "source_controller_sha256":digest(&source.assignment.controller.0),"behavior":source.report,
        "presentation":presentation.report,"particles":manifest["particles"],"attachments":manifest["attachments"]["conversion"],
        "prepared_recipe_sha256":digest(encoded.as_bytes()),"baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes())});
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
