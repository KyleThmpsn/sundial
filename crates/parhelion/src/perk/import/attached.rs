//! Private attachment imports retain separate controller and entity lifetimes.
use super::*;
use parhelion_import::d2_mot::{gameplay::perks::attached as source, native::attachment};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::sandbox_perk::program::{
    Asset, AttachmentTarget, ImportedAsset, NativeGroup, Position, native_draft,
};

const ROOT: u32 = 0x80BC57DD;
const EMPTY: u32 = 0x811C9DC5;

fn attach(
    source: &lower::attachment::Attachment,
    graph: u32,
    namespace: &str,
    retained_graphs: &BTreeSet<u32>,
    timed_graphs: &BTreeMap<u32, f32>,
) -> Result<Action, String> {
    if !source.retained || source.stacks_name != EMPTY {
        return Err("Attachment retention or creation stacks require translation".into());
    }
    let lifetime = match source.cleanup {
        lower::attachment::Cleanup::Independent => independent_key(namespace)?,
        lower::attachment::Cleanup::Automatic | lower::attachment::Cleanup::UnlessParentIsHost
            if retained_graphs.contains(&source.graph) =>
        {
            EMPTY
        }
        lower::attachment::Cleanup::Automatic | lower::attachment::Cleanup::UnlessParentIsHost
            if timed_graphs.contains_key(&source.graph) =>
        {
            independent_key(namespace)?
        }
        lower::attachment::Cleanup::Automatic | lower::attachment::Cleanup::UnlessParentIsHost => {
            return Err(
                "Component-sensitive attachment cleanup still requires a qualified native lifetime"
                    .into(),
            );
        }
    };
    Ok(Action::Attach {
        asset: Asset {
            graph,
            ..Default::default()
        },
        mode: AttachmentTarget::from_byte(source.target),
        keys: [lifetime, source.removal_name],
        float_bits: [source.removal_value.to_bits(); 4],
    })
}

fn independent_key(namespace: &str) -> Result<u32, String> {
    let key = format!("{namespace}/independent-attachment")
        .bytes()
        .fold(EMPTY, |hash, byte| {
            hash.wrapping_mul(16777619) ^ u32::from(byte)
        });
    if matches!(key, 0 | u32::MAX | EMPTY) {
        return Err("The private attachment lifetime key is reserved".into());
    }
    Ok(key)
}

fn hosted(
    input: attachment::ActionInput<'_>,
    template: u32,
    namespace: &str,
) -> anyhow::Result<attachment::CompiledAction> {
    let source = source::hosted(input.reader, input.payload, input.selectors)?;
    let actions = source
        .attachments
        .iter()
        .map(|action| {
            let graph = input.graphs.get(&action.graph).ok_or_else(|| {
                anyhow::anyhow!("hosted attachment is outside the private closure")
            })?;
            attach(
                action,
                *graph,
                namespace,
                input.retained_graphs,
                input.timed_graphs,
            )
            .map_err(anyhow::Error::msg)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let program = Program {
        name: format!("Imported Controller {:08X}", input.source),
        trigger: Trigger::Native,
        native_trigger: Some(node(source.activation.node)),
        native_removal: Some(node(source.removal)),
        duration_ms: 0,
        cooldown_ms: 0,
        actions,
        additional_groups: vec![NativeGroup {
            activation: vec![NativeNode {
                kind: 0,
                bytes: vec![0, 0, 128, 63, 255, 0, 0, 0],
            }],
            effects: source.modifiers.into_iter().map(node).collect(),
            ..Default::default()
        }],
        ..Program::default()
    };
    // These selectors already use the closure's private category dictionary.
    // Package compilation would resolve temporary tags as resident resources
    // and rebuild those masks against unrelated stock category indices.
    let draft = native_draft(&program).map_err(anyhow::Error::msg)?;
    let payload = Payload(draft.graph.emit().map_err(anyhow::Error::msg)?);
    Ok(attachment::CompiledAction { payload, template })
}

fn rename(manifest: &mut Value, prefix: &str) -> Result<(), String> {
    let name = |value: &str| format!("{prefix}-{value}");
    for section in ["particles", "attachments"] {
        for node in manifest[section]["nodes"]
            .as_array_mut()
            .ok_or("Imported nodes are absent")?
        {
            node["symbol"] = json!(name(
                node["symbol"]
                    .as_str()
                    .ok_or("Imported node symbol is absent")?
            ));
            if let Some(reference) = node["reference"].as_str() {
                node["reference"] = json!(name(reference));
            }
            for patch in node["patches"]
                .as_array_mut()
                .ok_or("Imported patches are absent")?
            {
                patch["symbol"] = json!(name(
                    patch["symbol"]
                        .as_str()
                        .ok_or("Imported patch symbol is absent")?
                ));
            }
        }
    }
    for root in manifest["attachments"]["roots"]
        .as_array_mut()
        .ok_or("Attachment roots are absent")?
    {
        *root = json!(name(
            root.as_str().ok_or("Attachment root symbol is absent")?
        ));
    }
    for system in manifest["particles"]["systems"]
        .as_object_mut()
        .ok_or("Particle systems are absent")?
        .values_mut()
    {
        *system = json!(name(
            system.as_str().ok_or("Particle system symbol is absent")?
        ));
    }
    Ok(())
}

pub(super) fn prepare(
    request: &Request<'_>,
    reader: &mut Reader,
    output: &Path,
    donor: &WeaponDonor,
    choices: Vec<HexHash>,
) -> Result<Prepared, String> {
    let source = source::extract(reader, request.plug_hash).map_err(error)?;
    let presentation = presentation::read(reader, request.plug_hash).map_err(error)?;
    let directory = output.join("attachments");
    fs::create_dir_all(&directory).map_err(error)?;
    let mut native =
        Reader::new(request.native_packages, &output.join("native"), false).map_err(error)?;
    let template = native
        .classes(0x808040B5)
        .into_iter()
        .min()
        .ok_or("Native action envelope is absent")?;
    let namespace = format!(
        "{}/perk/{:08X}/{}/{}",
        request.recipe.namespace, request.plug_hash, request.socket_index, request.choice_index
    );
    let mut roots = BTreeSet::new();
    let mut parameters = BTreeMap::new();
    for effect in &source.effects {
        match effect {
            source::Effect::Attach(action) => {
                roots.insert(action.graph);
                if let Some(previous) = parameters.insert(action.graph, action.parameters.clone())
                    && previous != action.parameters
                {
                    return Err(
                        "Root attachment callers supply different creation parameters".into(),
                    );
                }
            }
            source::Effect::Spawn(action) => {
                roots.insert(action.graph);
            }
        }
    }
    let mut converted = attachment::convert_gameplay(
        reader,
        &mut native,
        &attachment::Request {
            source_tag: source.assignment.action_tag,
            source_packages: request.modern_packages,
            native_packages: request.native_packages,
            directory: &directory,
            namespace: &namespace,
        },
        &roots.into_iter().collect::<Vec<_>>(),
        &parameters,
        &BTreeSet::new(),
        |input| hosted(input, template, &namespace),
    )
    .map_err(error)?;
    let prefix = format!(
        "{:08X}-{:08X}-{}-{}",
        request.profile_key, request.plug_hash, request.socket_index, request.choice_index
    );
    rename(&mut converted.manifest, &prefix)?;
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&converted.manifest).map_err(error)?,
    )
    .map_err(error)?;
    let sha256 = attachment::fingerprint(&directory).map_err(error)?;
    let mut actions = Vec::new();
    let mut imported_assets = Vec::new();
    for effect in &source.effects {
        let (graph, action) = match effect {
            source::Effect::Attach(action) => (
                action.graph,
                attach(
                    action,
                    ROOT,
                    &namespace,
                    &converted.retained_graphs,
                    &converted.timed_graphs,
                )?,
            ),
            source::Effect::Spawn(action) => (
                action.graph,
                Action::Spawn {
                    asset: Asset {
                        graph: ROOT,
                        ..Default::default()
                    },
                    position: if action.position == 1 {
                        Position::Event
                    } else {
                        Position::Owner
                    },
                },
            ),
        };
        imported_assets.push(ImportedAsset {
            action_index: actions.len(),
            directory: directory.clone(),
            sha256: sha256.clone(),
            symbol: format!(
                "{prefix}-{}",
                converted
                    .roots
                    .get(&graph)
                    .ok_or("Imported root graph is absent")?
            ),
        });
        actions.push(action);
    }
    let name = if request.name.trim().is_empty() {
        presentation.name.clone()
    } else {
        request.name.into()
    };
    let mut label_resources = converted.selectors.resources.clone();
    label_resources
        .tags
        .insert(converted.dictionary.0, 0x80C70CA1);
    let activation = source
        .activation
        .iter()
        .map(|&offset| {
            lower::kill_condition_with_labels(
                &source.assignment.controller,
                offset,
                &label_resources,
                &converted.selectors.groups,
            )
            .map(node)
            .map_err(error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut activation = activation.into_iter();
    let program = Program {
        name: name.clone(),
        trigger: Trigger::Native,
        native_trigger: activation.next(),
        alternative_triggers: activation.collect(),
        native_removal: Some(node(source.removal)),
        duration_ms: 0,
        cooldown_ms: 0,
        actions,
        imported_assets,
        imported_labels: Some(format!("{prefix}-{}", converted.dictionary.1)),
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
    native.finish().map_err(error)?;
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({"status":"experimental attachment candidate","gameplay_verified":false,
        "source_plug":request.plug_hash,"source_action":source.assignment.action_tag,
        "source_controller_sha256":digest(&source.assignment.controller.0),
        "presentation":presentation.report,"attachments":converted.manifest["attachments"]["conversion"],
        "prepared_recipe_sha256":digest(encoded.as_bytes()),
        "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes())});
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
