//! What the compiler writes: the converted clips and their native descriptor rows, the native
//! bank, states, pose layers and markers built on the carrier's, and the section's receipt.
use super::pairing::WeaponLocal;
use super::plan::{Routes, Sources};
use super::translation::{Key, Translation};
use super::*;

/// The converted clips and the native descriptor rows that play them.
pub(super) struct Clips {
    /// The first native descriptor ordinal the translated rows take, after the carrier's.
    pub(super) first: usize,
    /// Each complete authored variant has its own native descriptor.
    descriptors: BTreeMap<Key, u32>,
    keys: BTreeMap<(u32, u32), Key>,
    /// Clip placeholders the linker replaces, in bank order.
    pub(super) clips: Vec<u32>,
    pub(super) rows: Vec<native::Row>,
    pub(super) extra: Vec<Value>,
    pub(super) reports: Vec<Value>,
    pub(super) used_targets: BTreeSet<u32>,
}

impl Clips {
    /// Writes each routed clip once and numbers a native descriptor for each source descriptor
    /// and weapon-local descriptor pair, after the carrier's descriptors.
    pub(super) fn write(
        native: &mut Reader,
        graph: &Path,
        sources: &Sources,
        routes: &Routes,
        local: &WeaponLocal,
        translation: &mut Translation,
        donors: &mut cue::Donors<'_>,
    ) -> Result<Self> {
        let (controller, keys) = (&sources.controller, &translation.keys);
        let first = sources.carrier.descriptors()?;
        let mut descriptors = BTreeMap::<Key, u32>::new();
        let mut converted = BTreeMap::<Key, (usize, u16, u8)>::new();
        let mut clips = Vec::new();
        let mut rows = Vec::new();
        let mut extra = Vec::new();
        let mut reports = Vec::new();
        let mut used_targets = BTreeSet::new();
        for (&request, route) in &routes.routes {
            for &(descriptor, _) in route {
                let key = keys[&(request, descriptor)];
                if descriptors.contains_key(&key) {
                    continue;
                }
                let entry = &controller.descriptors[descriptor as usize];
                let (index, frames, composition) = if let Some(&found) = converted.get(&key) {
                    found
                } else {
                    let tag = controller.clips[key.0];
                    let Ok((result, mut report)) =
                        translation.authored.remove(&key).context("Routed clip")?
                    else {
                        anyhow::bail!("Routed clip {tag:08X} failed translation");
                    };
                    let file = match key.1 {
                        Some(weapon) => format!(
                            "animation/source-clip-{tag:08X}-{:08X}-{:08X}-{:08X}-{:08X}.bin",
                            weapon, key.3, key.4, key.2
                        ),
                        None => format!(
                            "animation/source-clip-{tag:08X}-{:08X}-{:08X}-{:08X}.bin",
                            key.3, key.4, key.2
                        ),
                    };
                    // The linker replaces each placeholder word in the bank. A second variant of
                    // one source clip takes a placeholder derived from its weapon-local descriptor.
                    let placeholder = if clips.contains(&tag) {
                        source::fnv(&format!(
                            "{tag:08X}-{:?}-{:08X}-{:08X}-{:08X}",
                            key.1, key.2, key.3, key.4
                        ))
                    } else {
                        tag
                    };
                    ensure!(
                        !clips.contains(&placeholder),
                        "Translated clip placeholder collision"
                    );
                    // Source cues native audio cannot resolve give way to the carrier's cues for
                    // the same action, timed by the weapon mechanism this clip carries.
                    let mut payload = result.payload;
                    if omitted_cues(&report["lowering"]) {
                        let actions = BTreeSet::from([key.2]);
                        let timing =
                            cue::authored(&payload, &sources.native_rig, local.native_handle)?;
                        report["cues"] = match donors.choose(
                            native,
                            &actions,
                            result.frames,
                            result.composition,
                        )? {
                            Some(donor) => {
                                let map = cue::retime(donor.timing, timing);
                                json!({"carrier_clip":format!("{:08X}",donor.tag),
                                    "carrier_timing":donor.timing.report(),"source_timing":timing.report(),
                                    "records":clip::carry_cues(&mut payload, &donor.payload, &map)?})
                            }
                            None => {
                                let named = cue::source_timed(
                                    &report["lowering"],
                                    &sources.carrier.vocabulary,
                                    key.2,
                                );
                                if named.is_empty() {
                                    json!({"carrier_clip":null,"reason":"No carrier clip of these actions carries cues"})
                                } else {
                                    json!({"carrier_clip":null,"timing":"source cue frames",
                                        "records":clip::name_cues(&mut payload, &named)?})
                                }
                            }
                        };
                    }
                    fs::write(graph.join(&file), &payload.0)?;
                    extra.push(json!({"source":placeholder,"template":sources.carrier.clip_template,"file":file}));
                    used_targets.extend(&result.targets);
                    report["file"] = json!(file);
                    report["placeholder"] = json!(placeholder);
                    report["authoring"] = result.report;
                    reports.push(report);
                    let found = (clips.len(), result.frames, result.composition);
                    clips.push(placeholder);
                    converted.insert(key, found);
                    found
                };
                descriptors.insert(key, u32::try_from(first + rows.len())?);
                rows.push(native::Row {
                    source: descriptor,
                    name: entry.name,
                    clip: index,
                    duration: (f32::from(frames.max(1)) - 1.) / 30.,
                    // Stock first-person descriptors play absolute clips in mode 1 and additive
                    // in 2.
                    mode: if composition == 0 { 1 } else { 2 },
                });
            }
        }
        Ok(Self {
            first,
            descriptors,
            keys: translation.keys.clone(),
            clips,
            rows,
            extra,
            reports,
            used_targets,
        })
    }

    /// The native descriptor `action`'s route plays for source `descriptor`.
    pub(super) fn native_descriptor(&self, request: u32, descriptor: u32) -> u32 {
        let key = self.keys[&(request, descriptor)];
        self.descriptors[&key]
    }
}

/// The native tables and files built on the carrier's.
pub(super) struct Tables {
    pub(super) files: BTreeMap<String, String>,
    pub(super) consumers: Vec<Value>,
    pub(super) rigs: Vec<Value>,
    pub(super) layer_routes: Vec<Value>,
    pub(super) layer_policy: Value,
    /// Actions that keep the carrier's state.
    pub(super) fallback: BTreeSet<String>,
}

impl Tables {
    /// Writes the converted bank, states and pose layers, the appended markers and the carrier
    /// resources the section links, and keys each file as the revision recorder does.
    pub(super) fn write(
        native: &mut Reader,
        graph: &Path,
        sources: &Sources,
        routes: &Routes,
        translation: &Translation,
        clips: &Clips,
    ) -> Result<Self> {
        let carrier = &sources.carrier;
        let native_routes = routes
            .actions
            .iter()
            .map(|(action, tree)| {
                (
                    *action,
                    tree.map(&mut |request| {
                        routes.routes[request]
                            .iter()
                            .map(|(descriptor, weight)| {
                                (clips.native_descriptor(*request, *descriptor), *weight)
                            })
                            .collect()
                    })
                    .reduced(),
                )
            })
            .collect::<native::Routes>();
        if sources.magazine_reload {
            ensure!(
                RELOADS
                    .iter()
                    .all(|(name, _)| native_routes.contains_key(&source::fnv(name))),
                "Magazine reload requires both translated source reload actions"
            );
        }
        let bank = native::bank(carrier, &clips.clips, &clips.rows)?;
        // Source supplemental layers are untranslated, and source loops such as `sprint_loop`
        // are held poses that rely on them. Each absolute translated descriptor therefore takes
        // the carrier's layer route for the actions that select it, the additive sway and
        // locomotion the carrier plays with its own clip for that action. Firing and settle
        // offsets authored against the donor's own actions are removed below. Actions that disagree,
        // additive rows and actions the carrier routes to no layer leave the descriptor without
        // one.
        let mut selecting = BTreeMap::<u32, BTreeSet<u32>>::new();
        for (&action, tree) in &native_routes {
            let mut choices = Vec::new();
            tree.leaves(&mut choices);
            for route in choices {
                for &(descriptor, _) in route {
                    selecting.entry(descriptor).or_default().insert(action);
                }
            }
        }
        let mut dispatch = Vec::with_capacity(clips.rows.len());
        let mut layer_routes = Vec::new();
        for (index, row) in clips.rows.iter().enumerate() {
            let ordinal = u32::try_from(clips.first + index)?;
            let mut found = BTreeSet::new();
            if row.mode == 1 {
                for &action in selecting.get(&ordinal).into_iter().flatten() {
                    found.insert(native::carrier_layer_route(carrier, native, action)?);
                }
            }
            let entry = match found.into_iter().collect::<Vec<_>>().as_slice() {
                [Some(entry)] => *entry,
                _ => u16::MAX,
            };
            dispatch.push(entry);
        }
        let layers = native::layers(carrier, &dispatch)?;
        let independent = clips
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.mode == 1)
            .map(|(index, _)| (clips.first + index, None))
            .collect();
        // Source actions are complete independent motion. The donor's settle, charge-fire and
        // firing offsets couple its own endpoints and must not displace these poses. Sprint and
        // procedural motion remain because source locomotion layers are not translated. The
        // shared writer preserves any layer that includes an absolute or replacement clip.
        let (layers, omitted, _, added) = crate::tiger::animation::layer::omit_coupled(
            native,
            &layers,
            &carrier.bank.1,
            &independent,
            &[0xFC23AA50, 0xD3B8D0AC, 0xE4E61EE3],
        )?;
        let written = layers.array(40, 2, Some(0x80800006))?;
        for (index, &original) in dispatch.iter().enumerate() {
            let ordinal = clips.first + index;
            let entry = layers.u16(written[ordinal])?;
            if entry != u16::MAX {
                layer_routes.push(
                    json!({"descriptor":ordinal,"layer_route":entry,"carrier_route":original}),
                );
            }
        }
        let layer_policy = json!({"omitted_additive_layers":omitted.iter().map(|n|format!("{n:08X}")).collect::<Vec<_>>(),
            "private_routes":added,"retained_sprint_layer":"4D467CC1","non_additive_layers_retained":true});
        let states = native::states(carrier, &native_routes)?;
        // Actions the source leaves untranslated keep the carrier's state.
        let fallback = routes
            .absent
            .iter()
            .cloned()
            .chain(
                translation
                    .unsupported
                    .iter()
                    .map(|(action, _)| format!("{action:08X}")),
            )
            .collect::<BTreeSet<_>>();
        // Hand targets the carrier does not publish are appended from the source placement.
        let appended = clips
            .used_targets
            .difference(&routes.carrier_markers)
            .copied()
            .collect::<BTreeSet<_>>();
        let markers = native::markers(
            carrier,
            &sources.placement,
            &sources.weapon_rig,
            &sources.extension.equipment_skeleton,
            &appended,
            graph,
        )?;
        let mut files = BTreeMap::new();
        let mut write = |key: &str, bytes: &[u8]| -> Result<()> {
            let file = format!("animation/first-person-{key}.bin");
            fs::write(graph.join(&file), bytes)?;
            files.insert(key.replace('-', "_"), file);
            Ok(())
        };
        write(
            "attachment-owner",
            &native.tag(carrier.attachment, Some(0x80809C36))?.0,
        )?;
        write("entity", &native.tag(carrier.entity, Some(0x80809C0F))?.0)?;
        write("lookup-owner", &carrier.lookup.1.0)?;
        write("bank", &carrier.bank.1.0)?;
        write("converted-bank", &bank.0)?;
        write("parameters-template", &carrier.parameters.1.0)?;
        write("parameters", &carrier.parameters.1.0)?;
        write("states-template", &carrier.states.1.0)?;
        write("states", &states.0)?;
        write("pose-layers-template", &carrier.layers.1.0)?;
        write("pose-layers", &layers.0)?;
        let mut consumers = Vec::new();
        for (index, (tag, payload)) in carrier.consumers.iter().enumerate() {
            let key = format!("consumer-{index}");
            write(&key, &payload.0)?;
            consumers.push(json!({"owner":tag,"file_key":key.replace('-', "_")}));
        }
        let mut rigs = sources.extension.owners.clone();
        rigs.push(markers);
        // Pinned payloads use the same keys the revision recorder derives from these rows.
        for (field, entries) in [("rigs", &rigs), ("extra_clips", &clips.extra)] {
            for (index, row) in entries.iter().enumerate() {
                files.insert(
                    format!("{field}_{index}"),
                    row["file"].as_str().context("Animation file")?.to_owned(),
                );
                if let Some(template) = row.get("template_file").and_then(Value::as_str) {
                    files.insert(
                        format!("{field}_{index}_template_file"),
                        template.to_owned(),
                    );
                }
            }
        }
        files.insert(
            "pose_layers_file".into(),
            "animation/first-person-pose-layers.bin".into(),
        );
        files.insert(
            "pose_layers_template_file".into(),
            "animation/first-person-pose-layers-template.bin".into(),
        );
        Ok(Self {
            files,
            consumers,
            rigs,
            layer_routes,
            layer_policy,
            fallback,
        })
    }
}

/// The compiled section's receipt: the linked files and native resources, the source it read,
/// every route, fallback, clip and pairing, and what stays native or untranslated.
pub(super) struct Receipt<'a> {
    pub(super) sources: &'a Sources,
    pub(super) routes: &'a Routes,
    pub(super) local: &'a WeaponLocal,
    pub(super) translation: &'a Translation,
    pub(super) clips: &'a Clips,
    pub(super) tables: &'a Tables,
    pub(super) retarget: &'a retarget::Retarget<'a>,
    pub(super) framing: Value,
}

impl Receipt<'_> {
    pub(super) fn section(self) -> Value {
        let Self {
            sources,
            routes,
            local,
            translation,
            clips,
            tables,
            retarget,
            framing,
        } = self;
        let (carrier, controller) = (&sources.carrier, &sources.controller);
        let descriptor_rows = clips
            .rows
            .iter()
            .map(|r| json!({"source":r.source,"name":format!("{:08X}",r.name),"clip":clips.clips[r.clip],"duration":r.duration,"mode":r.mode}))
            .collect::<Vec<_>>();
        let mut section = json!({
            "first_person_status": "linked",
            "runtime_entity": carrier.runtime,
            "first_person": {
                "controller": "marathon_source",
                "attachment_owner": carrier.attachment,
                "entity": carrier.entity,
                "lookup_owner": carrier.lookup.0,
                "bank": carrier.bank.0,
                "files": tables.files,
                "clips": [],
                "extra_clips": clips.extra,
                "bank_consumers": tables.consumers,
                "pose_layers": {"owner":carrier.pose.0,"tag":carrier.layers.0,
                    "file":"animation/first-person-pose-layers.bin","template_file":"animation/first-person-pose-layers-template.bin"},
                "state_conversion": {"status":"source_states","parameters":carrier.parameters.0,"states":carrier.states.0,
                    "first_translated_descriptor":clips.first,"native_fallback_states":tables.fallback},
                "rig_extension": true,
                "rigs": tables.rigs,
                "source": {
                    "weapon_definition_profile": format!("{:08X}", sources.profile),
                    "hold_class": format!("{:08X}", sources.hold),
                    "selection": sources.selection_tag,
                    "default_arm_entity": sources.arms_entity,
                    "runner_alternatives": sources.alternatives,
                    "arm_skeleton": sources.arm_skeleton_tag,
                    "arm_controls": sources.arm_controls_tag,
                    "action_lookup": sources.lookup_tag,
                    "driver": sources.arm_driver,
                    "bank": controller.bank_tag,
                    "fixed_builtins": sources.fixed.iter().map(|(b, v)| (b.to_string(), format!("{v:08X}"))).collect::<BTreeMap<_, _>>(),
                    "control_extensions": sources.arms.extensions,
                },
                "routes": routes.actions.iter().map(|(action, tree)| (format!("{action:08X}"), tree.map(&mut |request| routes.routes[request].iter().map(|(d,w)| json!({"source_descriptor":d,"native_descriptor":clips.native_descriptor(*request,*d),"weight":w})).collect::<Vec<_>>()).reduced())).collect::<BTreeMap<_, _>>(),
                "input_adaptation": {"dynamic_builtins":[1,10,12,18],
                    "policy":"Shadowkeep stance, awareness, sprint and ammunition names select source dictionary routes. Unknown native names and untranslated source inputs use the unset source value. Named reload actions retain their explicit ammunition input.",
                    "source_producers_verified":false},
                "absent_native_actions": routes.absent,
                // A shell-fed carrier requests reload_enter, a shell insert and chamber_1, so a
                // source weapon that reloads by magazine never plays its reload on it.
                "unrequested_source_reloads": RELOADS.iter().map(|(action, _)| source::fnv(action))
                    .filter(|action| controller.names.contains_key(action) && !routes.carrier_actions.contains(action))
                    .map(|action| format!("{action:08X}")).collect::<Vec<_>>(),
                "unsupported_actions": translation.unsupported.iter().map(|(action, reason)| json!({"action":format!("{action:08X}"),"reason":reason})).collect::<Vec<_>>(),
                "descriptors": descriptor_rows,
                "source_clips": clips.reports,
                "retarget": retarget.report,
                "framing": framing,
                "carrier_layer_routes": tables.layer_routes,
                "layer_policy": tables.layer_policy,
                "weapon_section": sources.extension.report,
                "arm_coverage": sources.coverage,
                "weapon_local": local.report,
                // Pairings of actions left out of the state table are not reported.
                "weapon_local_pairings": local.pairings.iter().filter(|(action, _)| routes.routes.contains_key(action)).map(|(_, row)| row).collect::<Vec<_>>(),
                "retained_native": [
                    "First-person entity, skeleton prefix, control definitions and limb solver",
                    "Bank clips and descriptors at their native ordinals, before the translated rows",
                    "States, parameter dictionary, pose layers and clips of the actions listed under state_conversion.native_fallback_states",
                    "Lookup look table at +EC and its shared selector at +F0, which address native clips by ordinal",
                    "Bank blend table at +84 and layer names at +58. The source bank names its own blend table, which is untranslated",
                    "Pose controller numeric parameters at +110, read by the native pose layers"
                ],
                "untranslated_source": [
                    "Supplemental pose layers and their numeric parameter programs. Absolute translated descriptors retain carrier locomotion and procedural layers, omitting incompatible additive firing offsets. Routes are listed under carrier_layer_routes",
                    "Weapon-local descriptors no named state, clip length or idle state reaches, listed under weapon_local. Length pairings rest on timing, not a source selection record",
                    "Attachment-local controllers and weapon-local clip events",
                    "Live hand contact and equipment pose timing for bone-local hand targets",
                    "Constraint rows with a target mode other than 3, which are disabled and listed per clip",
                    "Audio, effect and sequence-selector clip events, recorded per clip. A clip that omitted them carries the carrier's cues for its action, listed per clip under cues"
                ],
            },
            "gameplay_verified": false,
        });
        if sources.magazine_reload {
            section["reload"] = json!("magazine");
        }
        section
    }
}
