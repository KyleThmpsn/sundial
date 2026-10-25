//! Named ability actions reuse the clip, constraint, bank and state-route converters.
use super::*;
use crate::{
    d1_roi::ability::dependency::{self, Asset, Patch},
    tiger::animation::{bank, control, state},
};
use first_person::conditional;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct Choice {
    pub descriptor: usize,
    pub clip: u32,
    pub weight: u32,
}

#[derive(Debug, Serialize)]
pub struct Action {
    pub name: u32,
    pub choices: Vec<Choice>,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub lookup: u32,
    pub bank: u32,
    pub states: u32,
    pub actions: Vec<Action>,
    pub inputs: BTreeMap<u32, String>,
}

/// The player animation objects to receive private copies. The graph assembler must bind all
/// returned resources to this same player route, including both the lookup and pose consumer.
pub struct Target {
    pub lookup: u32,
    pub pose: u32,
    pub skeleton: u32,
    pub controls: u32,
    pub source_controls: u32,
    pub source_pose: u32,
}

pub struct Prepared {
    pub assets: Vec<Asset>,
    /// Native owner, field offset and required private resource. These are obligations for the
    /// containing player graph, not permission to modify stock resources.
    bindings: Vec<(u32, Patch)>,
    /// Sequence events reference action names, not the names inside clip payloads.
    pub actions: BTreeMap<u32, u32>,
    pub evidence: Value,
    roots: Vec<String>,
    owners: BTreeMap<u32, String>,
}

pub struct Bound {
    pub assets: Vec<dependency::Linked>,
    pub owners: BTreeMap<u32, Payload>,
    pub evidence: Value,
}

impl Prepared {
    pub fn bindings(&self) -> &[(u32, Patch)] {
        &self.bindings
    }

    /// Bind both native animation consumers to the same allocated private resources. Caller
    /// payloads remain untouched on failure. Their checked copies still require graph publication.
    pub fn bind(
        &self,
        owners: &BTreeMap<u32, Payload>,
        allocations: &BTreeMap<String, u32>,
    ) -> Result<Bound> {
        ensure!(
            owners.keys().eq(self.owners.keys()),
            "Ability animation consumer set differs"
        );
        for (tag, owner) in owners {
            ensure!(
                digest(owner) == self.owners[tag],
                "Ability animation consumer changed since preparation"
            );
        }
        ensure!(
            allocations.values().all(|tag| !owners.contains_key(tag)),
            "Private animation allocation collides with a consumer owner"
        );
        let assets = dependency::link(&self.assets, &self.roots, allocations)?;
        let classes = assets
            .iter()
            .map(|asset| (asset.symbol.as_str(), asset.class))
            .collect::<BTreeMap<_, _>>();
        let mut linked_owners = owners.clone();
        let mut written = BTreeSet::new();
        for (tag, patch) in &self.bindings {
            ensure!(
                classes.get(patch.symbol.as_str()) == Some(&patch.class)
                    && written.insert((*tag, patch.offset)),
                "Ability animation binding is duplicate or has the wrong class"
            );
            let owner = linked_owners
                .get_mut(tag)
                .context("Ability animation consumer is absent")?;
            crate::presentation::put(
                &mut owner.0,
                patch.offset,
                &allocations[&patch.symbol].to_le_bytes(),
            )?;
        }
        let evidence = json!({"owners":linked_owners.iter().map(|(tag, owner)| json!({
            "tag":tag,"source_sha256":self.owners[tag],"sha256":digest(owner)
        })).collect::<Vec<_>>(),"assets":assets.iter().map(|asset| json!({
            "symbol":asset.symbol,"tag":asset.tag,"class":asset.class,"sha256":digest(&asset.payload)
        })).collect::<Vec<_>>(),"published":false,"gameplay_verified":false});
        Ok(Bound {
            assets,
            owners: linked_owners,
            evidence,
        })
    }
}

fn digest(p: &Payload) -> String {
    hex::encode(Sha256::digest(&p.0))
}

pub fn read(source: &Source, lookup: u32, names: &BTreeSet<u32>) -> Result<Plan> {
    ensure!(!names.is_empty(), "Ability animation action set is empty");
    ensure!(
        source.reference(lookup)? == SOURCE_OWNER,
        "Source animation lookup owner type differs"
    );
    let owner = source.read(lookup)?;
    let definition = owner.pointer(24)?;
    ensure!(
        owner.u32(definition - 4)? == SOURCE_LOOKUP,
        "Source animation lookup class differs"
    );
    let bank_tag = owner.u32(definition + 0xA8)?;
    let states_tag = owner.u32(definition + 0xB4)?;
    ensure!(
        source.reference(bank_tag)? == SOURCE_BANK && source.reference(states_tag)? == 0x8080222A,
        "Source action bank or state table class differs"
    );
    let bank = source.read(bank_tag)?;
    let states = source.read(states_tag)?;
    let nodes = conditional::names(&states, true)?;
    let mut inputs = BTreeMap::from([
        (lookup, digest(&owner)),
        (bank_tag, digest(&bank)),
        (states_tag, digest(&states)),
    ]);
    let mut actions = Vec::new();
    for &name in names {
        let node = *nodes
            .get(&name)
            .with_context(|| format!("Source animation action {name:08X} is absent"))?;
        // Conditional player routes need an explicit parameter context. The weapon importer
        // already owns that route, and a subclass must not pick its first variant arbitrarily.
        ensure!(
            states.u64(node)? == 1,
            "Ability action needs a qualified selector context"
        );
        let action = conditional::action(&states, &bank, node, true, &BTreeMap::new())?;
        let mut choices = Vec::new();
        for choice in action.choices {
            ensure!(
                source.reference(choice.tag)? == SOURCE_CLIP,
                "Ability action names a non-clip resource"
            );
            inputs.insert(choice.tag, digest(&source.read(choice.tag)?));
            choices.push(Choice {
                descriptor: choice.descriptor,
                clip: choice.tag,
                weight: choice.weight,
            });
        }
        actions.push(Action { name, choices });
    }
    Ok(Plan {
        lookup,
        bank: bank_tag,
        states: states_tag,
        actions,
        inputs,
    })
}

/// Join selector values only through the same named action's ordered frame anchors. The source
/// frame and all other event bytes remain unchanged. This is an asset witness, not a claim that
/// the whole selector enumeration is shared. Disagreeing counts, times or repeated joins fail.
fn markers(
    source: &Payload,
    native: &Payload,
    mapping: &mut BTreeMap<u8, u8>,
) -> Result<(Payload, Value)> {
    let events = |p: &Payload, source: bool| -> Result<Vec<usize>> {
        let mut result = Vec::new();
        for row in p.array(
            if source { 0x140 } else { 0x160 },
            8,
            Some(if source { 0x808004E1 } else { 0x8080902B }),
        )? {
            let at = p.pointer(row)?;
            if p.u32(at - 4)? == if source { 0x80800436 } else { 0x8080903A } {
                ensure!(
                    p.u16(at + 2)? == 2 && p.u8(at + 5)? == 0,
                    "Action marker flags differ"
                );
                result.push(at);
            }
        }
        Ok(result)
    };
    let from = events(source, true)?;
    let to = events(native, false)?;
    ensure!(from.len() == to.len(), "Named action marker counts differ");
    let mut output = source.clone();
    let mut evidence = Vec::new();
    for (a, b) in from.into_iter().zip(to) {
        ensure!(
            source.i16(a)? == native.i16(b)?,
            "Named action marker frame anchors differ"
        );
        let before = source.u8(a + 4)?;
        let after = native.u8(b + 4)?;
        ensure!(
            mapping
                .iter()
                .all(|(&from, &to)| from == before || to != after),
            "Named action marker translations merge distinct selectors"
        );
        if let Some(previous) = mapping.insert(before, after) {
            ensure!(
                previous == after,
                "Named action marker translations disagree"
            );
        }
        output.0[a + 4] = after;
        evidence.push(json!({"frame":source.i16(a)?,"source":before,"native":after}));
    }
    Ok((output, json!(evidence)))
}

/// Prepare all selected actions atomically in memory. Existing state and descriptor rows stay
/// intact. All selected source clips must convert and all timed resources must be supplied as a
/// complete private dependency closure. No subset or native motion fallback is returned.
pub fn prepare(
    source: &Source,
    native: &mut Reader,
    plan: &Plan,
    target: &Target,
    prefix: &str,
    resources: &BTreeMap<u32, String>,
    dependencies: &[Asset],
) -> Result<Prepared> {
    dependency::symbol(prefix)?;
    ensure!(
        native.is_native(),
        "Ability animation requires Shadowkeep packages"
    );
    let rig_evidence = super::super::rig::validate_target(source, native, target.skeleton)?;
    let names = plan
        .actions
        .iter()
        .map(|action| action.name)
        .collect::<BTreeSet<_>>();
    let refreshed = read(source, plan.lookup, &names)?;
    ensure!(
        serde_json::to_value(&refreshed)? == serde_json::to_value(plan)?,
        "Source animation plan changed"
    );
    let lookup = native.tag(target.lookup, Some(0x80809C36))?;
    let pose = native.tag(target.pose, Some(0x80809C36))?;
    let lp = lookup.pointer(24)?;
    let pp = pose.pointer(24)?;
    ensure!(
        lookup.u32(lp - 4)? == 0x8080344B
            && pose.u32(pp - 4)? == 0x80803640
            && lookup.u32(lp + 0x90)? == pose.u32(pp + 0x108)?,
        "Native ability animation consumers disagree"
    );
    let bank = native.tag(lookup.u32(lp + 0x90)?, Some(0x808036F6))?;
    let states = native.tag(lookup.u32(lp + 0x9C)?, Some(0x80803465))?;
    let layers = native.tag(pose.u32(pp + 0x11C)?, Some(0x808034EB))?;
    let directions = native.tag(pose.u32(pp + 0x114)?, Some(0x8080348E))?;
    let skeleton = native.tag(target.skeleton, Some(0x80809C36))?;
    let controls = native.tag(target.controls, Some(0x80809C36))?;
    let rig = control::Rig::read_character(&skeleton, &controls)?;
    let source_controls = source.read(target.source_controls)?;
    let defaults = character::control_defaults(&source_controls, &controls, &rig)?;
    let source_bank = source.read(plan.bank)?;
    let source_descriptors = source_bank.array(0x68, 32, Some(0x80800577))?;
    let source_pose = source.read(target.source_pose)?;
    let sp = source_pose.pointer(24)?;
    ensure!(
        source_pose.u32(sp - 4)? == 0x80802790 && source_pose.u32(sp + 0x108)? == plan.bank,
        "Source ability pose and lookup do not share their bank"
    );
    let source_layers_tag = source_pose.u32(sp + 0x118)?;
    let source_directions_tag = source_pose.u32(sp + 0x110)?;
    ensure!(
        source.reference(source_layers_tag)? == 0x80802919
            && source.reference(source_directions_tag)? == 0x8080221B,
        "Source ability pose dispatch classes differ"
    );
    let source_layers = source.read(source_layers_tag)?;
    let source_directions = source.read(source_directions_tag)?;
    let dispatches = [
        (
            &source_layers,
            source_layers.array(40, 2, Some(0x80800006))?,
        ),
        (
            &source_directions,
            source_directions.array(8, 2, Some(0x80800006))?,
        ),
        (
            &source_directions,
            source_directions.array(0x38, 2, Some(0x80800006))?,
        ),
    ];
    for (payload, rows) in &dispatches {
        ensure!(
            rows.len() == source_descriptors.len(),
            "Source pose dispatch does not cover its bank"
        );
        for choice in plan.actions.iter().flat_map(|action| &action.choices) {
            ensure!(
                payload.u16(
                    *rows
                        .get(choice.descriptor)
                        .context("Source pose descriptor")?
                )? == u16::MAX,
                "Ability action has source supplemental pose or direction dependencies"
            );
        }
    }
    let native_descriptors = bank.array(0x68, 32, Some(0x80809002))?;
    for &row in &native_descriptors {
        ensure!(
            bank.bytes::<16>(row)? == [0; 16],
            "Native ability bank has inline descriptor data that needs relocation"
        );
    }
    let nodes = conditional::names(&states, false)?;
    let private_actions = names
        .iter()
        .map(|&name| {
            (
                name,
                super::super::weapon::key(&format!("{prefix}-action-{name:08X}")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut assets = Vec::new();
    let mut additions = Vec::new();
    let mut routes = state::ChoiceRoutes::new();
    let mut descriptors = Vec::new();
    let mut selector_mapping = BTreeMap::new();
    let mut evidence = Vec::new();
    let mut used_resources = BTreeSet::new();
    for action in &plan.actions {
        let node = *nodes
            .get(&action.name)
            .context("Native action witness is absent")?;
        ensure!(
            states.u64(node)? == 1,
            "Native ability action needs a selector context"
        );
        let witness = conditional::action(&states, &bank, node, false, &BTreeMap::new())?;
        ensure!(
            action.choices.len() == witness.choices.len()
                && action
                    .choices
                    .iter()
                    .zip(&witness.choices)
                    .all(|(a, b)| a.weight == b.weight),
            "Named action weighted variants differ"
        );
        let library =
            event::Library::selected(native, &witness.choices.iter().map(|c| c.tag).collect())?;
        let mut weights = Vec::new();
        for (choice, template) in action.choices.iter().zip(witness.choices) {
            let original = source.read(choice.clip)?;
            let counterpart = native.tag(template.tag, Some(0x80808F49))?;
            ensure!(
                usize::from(original.u8(0x12A)?) == rig.bind.len()
                    && usize::from(original.u8(0x12B)?) == rig.defaults.len()
                    && original.u32(0x164)? == counterpart.u32(0x180)?,
                "Ability clip control counts or composition differ"
            );
            let groups = original.array(0x150, 8, Some(0x80800380))?;
            let native_groups = counterpart.array(0x170, 8, Some(0x8080907F))?;
            ensure!(
                groups.len() == native_groups.len()
                    && groups
                        .iter()
                        .zip(native_groups)
                        .all(|(&a, b)| original.0[a..a + 8] == counterpart.0[b..b + 8]),
                "Ability clip control groups differ"
            );
            let (qualified, marker_evidence) =
                markers(&original, &counterpart, &mut selector_mapping)?;
            library.validate_source(source, &qualified)?;
            let bones = (0..u16::try_from(rig.bind.len())?).collect::<Vec<_>>();
            let (mut converted, mut report) =
                convert_events(&qualified, Some((&counterpart, &bones)), Some(&library))?;
            if converted.u8(0x181)? == 0 {
                converted = crate::tiger::animation::clip::bind_defaults(&converted, &defaults)?;
            }
            for frame in [0, usize::from(converted.u16(0x13C)?) - 1] {
                crate::tiger::animation::clip::sample(&converted, &rig.defaults, frame)?;
            }
            let mut patches = Vec::new();
            for event in report["audio"]["events"]
                .as_array_mut()
                .context("Ability clip events")?
            {
                let Some(tag) = event["source_resource"].as_u64() else {
                    continue;
                };
                let tag = u32::try_from(tag)?;
                let symbol = resources
                    .get(&tag)
                    .context("Ability clip has an untranslated effect or sound dependency")?;
                let class = match source.reference(tag)? {
                    0x8080080A => 0x80809802,
                    0x80800734 => 0x80809C0F,
                    _ => anyhow::bail!("Ability clip dependency class is unsupported"),
                };
                let at = usize::try_from(
                    event["native_offset"]
                        .as_u64()
                        .context("Ability event offset")?,
                )? + 24;
                put(&mut converted.0, at, &u32::MAX.to_le_bytes());
                patches.push(Patch {
                    offset: at,
                    symbol: symbol.clone(),
                    class,
                });
                event["native_witness_resource"] = event["native_resource"].take();
                event["private_resource"] = json!(symbol);
                used_resources.insert(tag);
            }
            report["audio"]["binding"] = json!("native_dispatch_witness_with_private_resources");
            let sr = *source_descriptors
                .get(choice.descriptor)
                .context("Source action descriptor")?;
            let nr = *native_descriptors
                .get(template.descriptor)
                .context("Native action descriptor")?;
            ensure!(
                source_bank.bytes::<6>(sr + 26)? == bank.bytes::<6>(nr + 26)?,
                "Action descriptor playback policies differ"
            );
            let index = assets.len();
            let symbol = format!("{prefix}-clip-{index}");
            additions.push(bank::Addition {
                descriptor: template.descriptor,
                template: template.tag,
                clip: 0x7D000000u32
                    .checked_add(u32::try_from(index)?)
                    .context("Ability clip identity overflow")?,
                duration: source_bank.f32(sr + 20)?,
            });
            weights.push((native_descriptors.len() + index, choice.weight));
            descriptors.push(source_bank.bytes::<8>(sr + 16)?);
            evidence.push(
                json!({"action":action.name,"source_clip":choice.clip,"native_witness":template.tag,
                "source_sha256":digest(&original),"native_witness_sha256":digest(&counterpart),
                "marker_translation":marker_evidence,"conversion":report}),
            );
            assets.push(Asset {
                symbol,
                class: 0x80808F49,
                payload: converted,
                patches,
            });
        }
        routes.insert(action.name, BTreeMap::from([(0, weights)]));
    }
    ensure!(
        !additions.is_empty(),
        "Ability animation preparation has no clips"
    );
    ensure!(
        used_resources == resources.keys().copied().collect(),
        "Ability resource mappings contain unused or missing entries"
    );
    let mut extension = bank::extend(
        &bank,
        &layers,
        &directions,
        &bank,
        &layers,
        &directions,
        &additions,
    )?;
    let rows = extension.bank.array(0x68, 32, Some(0x80809002))?;
    for (descriptor, data) in extension.descriptors.iter().zip(descriptors) {
        put(&mut extension.bank.0, rows[*descriptor] + 16, &data);
    }
    let states = state::extend_actions(&states, &routes, &private_actions)?;
    let clips = extension.bank.array(8, 4, Some(0x80808F48))?;
    let first = clips.len() - assets.len();
    let mut patches = Vec::new();
    for (row, asset) in clips[first..].iter().zip(&assets) {
        put(&mut extension.bank.0, *row, &u32::MAX.to_le_bytes());
        patches.push(Patch {
            offset: *row,
            symbol: asset.symbol.clone(),
            class: asset.class,
        });
    }
    let mut bindings = Vec::new();
    let mut roots = Vec::new();
    for (name, class, payload, patches, fields) in [
        (
            "bank",
            0x808036F6,
            extension.bank,
            patches,
            vec![(target.lookup, lp + 0x90), (target.pose, pp + 0x108)],
        ),
        (
            "actions",
            0x80803465,
            states,
            Vec::new(),
            vec![(target.lookup, lp + 0x9C)],
        ),
        (
            "layers",
            0x808034EB,
            extension.layers,
            Vec::new(),
            vec![(target.pose, pp + 0x11C)],
        ),
        (
            "directions",
            0x8080348E,
            extension.directions,
            Vec::new(),
            vec![(target.pose, pp + 0x114)],
        ),
    ] {
        let symbol = format!("{prefix}-{name}");
        for (owner, offset) in fields {
            bindings.push((
                owner,
                Patch {
                    offset,
                    symbol: symbol.clone(),
                    class,
                },
            ));
        }
        roots.push(symbol.clone());
        assets.push(Asset {
            symbol,
            class,
            payload,
            patches,
        });
    }
    assets.extend_from_slice(dependencies);
    let closure = dependency::validate(&assets, &roots)?;
    source.unchanged()?;
    Ok(Prepared {
        assets,
        bindings,
        actions: private_actions,
        roots,
        owners: BTreeMap::from([
            (target.lookup, digest(&lookup)),
            (target.pose, digest(&pose)),
        ]),
        evidence: json!({"source":plan,"clips":evidence,"closure":closure,
            "rig":rig_evidence,
            "source_controls_sha256":digest(&source_controls),"native_controls_sha256":digest(&controls),
            "source_pose_sha256":digest(&source_pose),"source_layers_sha256":digest(&source_layers),
            "source_directions_sha256":digest(&source_directions),
            "binding_required":true,"gameplay_verified":false}),
    })
}
