//! Private attachment imports retain separate controller and entity lifetimes.
use super::*;
use parhelion_import::d2_mot::{
    gameplay::perks::attached::{self as source, attach_action, compile_hosted},
    native::attachment,
};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::sandbox_perk::program::{Asset, ImportedAsset, Position};

const ROOT: u32 = 0x80BC57DD;

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
        |input| compile_hosted(input, template, &namespace),
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
                attach_action(
                    action,
                    ROOT,
                    &namespace,
                    &converted.retained_graphs,
                    &converted.timed_graphs,
                )
                .map_err(error)?,
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
            graph: None,
        });
        actions.push(action);
    }
    let name = plug_name(request, &presentation);
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
    let mut effect = super::super::PerkRecipe::effect(request.source_perk_index);
    effect.program = Some(program);
    reader.finish().map_err(error)?;
    native.finish().map_err(error)?;
    let provenance = json!({"status":"experimental attachment candidate","gameplay_verified":false,
        "source_plug":request.plug_hash,"source_action":source.assignment.action_tag,
        "source_controller_sha256":sha256_hex(&source.assignment.controller.0),
        "presentation":presentation.report,"attachments":converted.manifest["attachments"]["conversion"]});
    let plug = Plug {
        presentation: &presentation,
        name,
        sandbox_perks: vec![effect],
        projectile: None,
    };
    finish(request, output, donor, choices, plug, |_| {}, provenance)
}
