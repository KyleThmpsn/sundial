//! Effect-only attachment groups with checked native registration and private resources.
use super::projectile::{
    assets::{Assets, metadata, retag},
    compatibility,
};
use crate::d2_mot::{
    entity::{
        compiled, context,
        links::{Graph, Object},
        sequence, transform,
    },
    native::effects::controller,
    particles,
    payload::Payload,
    reader::{Reader, write_json},
    tfx,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod audio;

pub struct Request<'a> {
    pub source_tag: u32,
    pub source_packages: &'a Path,
    pub native_packages: &'a Path,
    pub directory: &'a Path,
    pub namespace: &'a str,
}

/// Pin every declared payload together with the manifest. The same check runs before
/// and after loading a group, so changed files cannot silently replace a prepared perk.
pub fn fingerprint(directory: &Path) -> Result<String> {
    let root = directory.canonicalize()?;
    let bytes = std::fs::read(root.join("asset-graph.json"))?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest["attachments"]["installable"] == true,
        "attachment group is not installable"
    );
    let mut digest = Sha256::new();
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    let mut files = BTreeSet::new();
    for section in ["particles", "attachments"] {
        for node in manifest[section]["nodes"].as_array().into_iter().flatten() {
            let name = node["file"].as_str().context("attachment payload path")?;
            let path = Path::new(name);
            ensure!(
                !path.as_os_str().is_empty()
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "attachment path leaves its group"
            );
            files.insert(path.to_owned());
        }
    }
    ensure!(!files.is_empty(), "attachment group has no payloads");
    for name in files {
        let path = root.join(&name).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "attachment payload escapes its group"
        );
        let bytes = std::fs::read(path)?;
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(hex::encode(digest.finalize()))
}

struct Component {
    source: u32,
    class: u32,
    template_tag: u32,
    template: Payload,
    entity: Payload,
    owner: u32,
    allocation: u32,
    payload: Option<Payload>,
}

fn put(p: &mut Payload, at: usize, value: &[u8]) -> Result<()> {
    p.0.get_mut(
        at..at
            .checked_add(value.len())
            .context("attachment offset overflow")?,
    )
    .context("attachment write outside payload")?
    .copy_from_slice(value);
    Ok(())
}

fn event_resources(
    source: &Reader,
    owner: &Payload,
    systems: &mut BTreeSet<u32>,
    sounds: &mut BTreeSet<u32>,
) -> Result<()> {
    let definition = owner.pointer(24)?;
    for row in owner.array(definition + 0x1D8, 24, Some(0x808091F1))? {
        let at = owner.pointer(row + 16)?;
        match owner.u32(at - 4)? {
            0x808067B9 => {
                for row in owner.array(at + 0x28, 24, Some(0x808067BB))? {
                    for field in sequence::presentation::fields(owner, row)? {
                        systems.insert(owner.u32(field)?);
                    }
                }
            }
            0x80806640 => {
                sounds.insert(source.ref64(owner, at + 0x50)?);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn convert(source: &mut Reader, native: &mut Reader, request: &Request<'_>) -> Result<Value> {
    let Request {
        source_tag,
        source_packages,
        native_packages,
        directory,
        namespace,
    } = *request;
    let entity = source.tag(source_tag, Some(0x80809AD8))?;
    let mut graph = Graph::read(&entity, true)?;
    let mut assets = Assets::default();
    let root = assets.reserve(native, "attachment-visual".into())?;
    let mut owners = BTreeMap::new();
    let mut components = Vec::new();
    let mut providers = BTreeMap::new();
    let mut systems = BTreeSet::new();
    let mut source_sounds = BTreeSet::new();
    for &tag in &graph.components {
        let owner = source.tag(tag, Some(0x80809B06))?;
        let definition = owner.pointer(24)?;
        let class = owner.u32(definition - 4)?;
        let (template_tag, parent) = match class {
            0x80808159 => (0x80C70B84, 0x80BC57DD),
            0x80808179 => (0x80F2E50A, 0x80BBB972),
            0x80809597 => (0x815282E0, 0x815282E1),
            0x80802AB8 => (0x80C709E8, 0x80B8228A),
            _ => anyhow::bail!("unsupported effect attachment component {class:08X}"),
        };
        if class == 0x80808179 {
            event_resources(source, &owner, &mut systems, &mut source_sounds)?;
        }
        let template = native.tag(template_tag, None)?;
        metadata(source, &[&owner], &mut providers)?;
        metadata(native, &[&template], &mut providers)?;
        components.push(Component {
            source: tag,
            class,
            template_tag,
            template: (*template).clone(),
            entity: (*native.tag(parent, Some(0x80809C0F))?).clone(),
            owner: assets.reserve(native, format!("attachment-owner-{tag:08X}"))?,
            allocation: assets.reserve(native, format!("attachment-allocation-{tag:08X}"))?,
            payload: None,
        });
        owners.insert(tag, (*owner).clone());
    }
    graph.validate_owners(&owners)?;
    ensure!(!systems.is_empty(), "attachment has no source particles");
    let render_inputs = directory.join("render-inputs");
    for (packages, modern, era) in [
        (source_packages, true, "modern"),
        (native_packages, false, "native"),
    ] {
        let folder = render_inputs.join(format!("tfx-{era}"));
        std::fs::create_dir_all(&folder)?;
        let mut reader = Reader::new(packages, &folder, modern)?;
        write_json(
            &folder.join("context.json"),
            &tfx::context(&mut reader, modern)?,
        )?;
        reader.finish()?;
    }
    let decompiler = super::automatic::decompiler(&mut |_| {})?;
    let particles = particles::native::convert(&particles::native::Request {
        source_packages,
        native_packages,
        render_inputs: &render_inputs,
        decompiler: &decompiler,
        graph: directory,
        work: &directory.join("particle-conversion"),
        systems: &systems.into_iter().collect::<Vec<_>>(),
        inputs: &BTreeMap::new(),
        template: 0x80EF30DB,
    })?;
    ensure!(
        particles["systems"]
            .as_object()
            .is_some_and(|systems| !systems.is_empty()),
        "no source attachment particles could be translated"
    );
    let template_sequence = native.tag(0x80F2E50A, None)?;
    let mut sound = None;
    for at in (0..template_sequence.0.len().saturating_sub(3)).step_by(4) {
        let tag = template_sequence.u32(at)?;
        if native.reference(tag).ok() == Some(0x80809802) {
            sound = Some(tag);
            break;
        }
    }
    let sound = sound.context("native effect compatibility sound")?;
    let mut differences = Vec::new();
    let mut pending_audio = assets.clone();
    let sounds = match audio::convert(
        source,
        native,
        &mut pending_audio,
        &source_sounds,
        sound,
        request,
    ) {
        Ok((sounds, report)) => {
            assets = pending_audio;
            differences.push(report);
            sounds
        }
        Err(error) => {
            differences.push(
                json!({"difference":"Audio uses a native compatibility cue.",
            "source_cues":source_sounds,"reason":format!("{error:#}")}),
            );
            BTreeMap::new()
        }
    };
    let bank_indices = components
        .iter()
        .enumerate()
        .filter(|(_, c)| c.class == 0x80809597)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    ensure!(
        bank_indices.len() == 1,
        "effect attachment needs one channel bank"
    );
    let c = &components[bank_indices[0]];
    let bank_allocation = native.tag(c.template.u32(0x44)?, None)?;
    let bank = controller::bank(
        &owners[&c.source],
        &c.template,
        &bank_allocation,
        c.owner,
        c.allocation,
    )?;
    let mut bank_parts = Some((bank.owner.clone(), bank.allocation.clone()));
    let mut objects = BTreeMap::<Object, Object>::new();
    for mapping in &bank.objects {
        objects.insert(mapping.source, mapping.target);
    }
    for component in &mut components {
        let original = &owners[&component.source];
        let allocation_tag = component.template.u32(0x44)?;
        let allocation = native.tag(allocation_tag, None)?;
        let (payload, allocation_payload, mappings) = match component.class {
            0x80809597 => {
                let (owner, allocation) = bank_parts.take().context("duplicate attachment bank")?;
                (owner, allocation, Vec::new())
            }
            0x80808159 => {
                let mut owner = retag(&component.template, component.owner)?;
                put(&mut owner, 0x44, &component.allocation.to_le_bytes())?;
                let original_allocation = source.tag(original.u32(0x84)?, None)?;
                let objects = context::effect::interfaces(
                    original,
                    &owner,
                    &original_allocation,
                    &allocation,
                    &providers,
                )?;
                (owner, (*allocation).clone(), objects)
            }
            0x80802AB8 => {
                let converted = transform::emit(
                    original,
                    &component.template,
                    &allocation,
                    &providers,
                    &graph,
                    component.owner,
                    component.allocation,
                )?;
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80808179 => {
                let (adjusted, mut bindings, report) = compatibility::sequence(
                    original,
                    &particles,
                    native,
                    &mut assets,
                    namespace,
                    0.0,
                    sound,
                )?;
                differences.extend(report.into_iter().filter(|row| {
                    sounds.is_empty()
                        || row["difference"]
                            != "Projectile audio uses the native compatibility cue."
                }));
                for event in original.array(original.pointer(24)? + 0x1D8, 24, Some(0x808091F1))? {
                    let at = original.pointer(event + 16)?;
                    if original.u32(at - 4)? == 0x80806640
                        && let Some(tag) = sounds.get(&source.ref64(original, at + 0x50)?)
                    {
                        bindings.resources.insert(
                            at + 0x50,
                            sequence::Resource {
                                source_class: 0x80809738,
                                native_class: 0x80809802,
                                tag: *tag,
                            },
                        );
                    }
                }
                let converted = sequence::emit(
                    &adjusted,
                    &component.template,
                    &allocation,
                    &bindings,
                    component.owner,
                    component.allocation,
                )?;
                graph = converted.link(&graph, &[&bank])?;
                (converted.owner, converted.allocation, converted.objects)
            }
            _ => unreachable!(),
        };
        for mapping in mappings {
            if let Some(previous) = objects.insert(mapping.source, mapping.target) {
                ensure!(
                    previous == mapping.target,
                    "conflicting attachment endpoint mapping"
                );
            }
        }
        assets.push(
            component.allocation,
            allocation_tag,
            allocation_payload,
            None,
        )?;
        assets.push(
            component.owner,
            component.template_tag,
            payload.clone(),
            (native.reference(component.template_tag)? == allocation_tag)
                .then_some(component.allocation),
        )?;
        component.payload = Some(payload);
    }
    let native_owners = components
        .iter()
        .map(|c| {
            Ok((
                c.owner,
                c.payload.clone().context("missing attachment owner")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let target_components = components.iter().map(|c| c.owner).collect::<Vec<_>>();
    let rows = graph.native_rows(
        &target_components,
        &native_owners,
        |object| {
            objects
                .get(&object)
                .copied()
                .with_context(|| format!("untranslated attachment endpoint {object:?}"))
        },
        |_, channel| Ok(channel),
        |_, _| anyhow::bail!("attachment external selector requires translation"),
    )?;
    let template = native.tag(0x80BC57DD, Some(0x80809C0F))?;
    let assembled = compiled::emit(
        &template,
        &components
            .iter()
            .map(|c| compiled::Component {
                owner: c.owner,
                payload: &native_owners[&c.owner],
                template: &c.template,
                entity: &c.entity,
            })
            .collect::<Vec<_>>(),
        &rows,
    )?;
    Graph::read(&assembled, false)?.validate_owners(&native_owners)?;
    assets.push(root, 0x80BC57DD, assembled, None)?;
    let mut attachment = assets.write(
        directory,
        root,
        json!({"source":source_tag,"differences":differences,"gameplay_verified":false}),
    )?;
    attachment["roots"] = json!([attachment["root"]]);
    attachment
        .as_object_mut()
        .context("attachment manifest")?
        .remove("root");
    Ok(json!({"particles":particles,"attachments":attachment}))
}
