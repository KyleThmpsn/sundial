//! Source weapon art on a native runtime that preserves every source bone binding.
use super::*;
use crate::GraphReference;
mod firing;
mod hud;
mod marker;
mod ornament;
mod stats;

/// A source weapon's presentation content: its entity, its unique content owner (resource class
/// `8080248D`) and the `0x140`-byte row its pattern's content key selects among the keyed
/// `80802312` rows at `+1F0`, keyed at `+18`, or the default row at `+B0`.
struct Content {
    entity: Payload,
    owner: Payload,
    row: usize,
    key: u32,
    keyed: bool,
}

fn content_row(source: &Source, export: &Value) -> Result<Content> {
    let (_, patterns) = source.table(0x8080339A)?;
    let index = usize::try_from(
        export["runtime"]["pattern_index"]
            .as_u64()
            .context("Source pattern index")?,
    )?;
    let row = *patterns
        .array(8, 32, Some(0x808033BC))?
        .get(index)
        .context("Source pattern")?;
    let key = patterns.u32(row + 4)?;
    ensure!(
        export["runtime"]["content_key"].as_u64() == Some(u64::from(key)),
        "Source content selector changed"
    );
    let root = u32::from_str_radix(
        export["runtime"]["runtime_entity"]
            .as_str()
            .context("Source weapon entity")?,
        16,
    )?;
    let entity = source.read(root)?;
    let mut selected = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let owner = source.read(entity.u32(r)?)?;
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? != 0x8080248D {
            continue;
        }
        let matches = owner
            .array(d + 0x1F0, 0x140, Some(0x80802312))?
            .into_iter()
            .filter(|&r| owner.u32(r + 24).ok() == Some(key))
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "Source content key is ambiguous");
        let (row, keyed) = matches.first().map_or((d + 0xB0, false), |&r| (r, true));
        selected.push((owner, row, keyed));
    }
    let Ok([(owner, row, keyed)]) = <[_; 1]>::try_from(selected) else {
        anyhow::bail!("Source has no unique content owner");
    };
    Ok(Content {
        entity,
        owner,
        row,
        key,
        keyed,
    })
}

pub(super) fn key(text: &str) -> u32 {
    let value = text.bytes().fold(0x811C9DC5u32, |h, b| {
        h.wrapping_mul(16777619) ^ u32::from(b)
    });
    if matches!(value, 0 | u32::MAX | 0x811C9DC5) {
        value ^ 0x10000
    } else {
        value
    }
}

/// The native art selector of a source weapon selector. Rise of Iron and Shadowkeep share
/// assignment keys at selectors 0 through 5 and 13 through 21 with the same numbers. Selector 6
/// holds the ammunition belt of the belt-fed layout `0, 1, 3, 4, 5, 6, 21`, which every source
/// row using it has and no source row combines with selector 2. Shadowkeep draws that layout as
/// `0, 1, 2, 3, 4, 5, 21`, and no stock row draws selector 6, so a belt left there never appears.
fn native_selector(source: u64) -> u64 {
    if source == 6 { 2 } else { source }
}

/// Source arrangement rows on the base's art template, with native selectors. Each slot keeps its
/// source selector beside the native one.
pub(super) fn native_rows(rows: &mut [Value], template: u16) -> Result<()> {
    for row in rows {
        row["template_index"] = json!(template);
        let slots = row["slots"]
            .as_array_mut()
            .context("Source weapon selectors")?;
        let mut native = BTreeSet::new();
        for slot in slots {
            let source = slot["selector"]
                .as_u64()
                .context("Source weapon selector")?;
            ensure!(
                native.insert(native_selector(source)),
                "Two source selectors share native selector {}",
                native_selector(source)
            );
            slot["source_selector"] = json!(source);
            slot["selector"] = json!(native_selector(source));
        }
    }
    Ok(())
}

/// The Shadowkeep inventory slot and ammunition of a Rise of Iron weapon in its source `slot`,
/// the client definition's `+18` byte, with damage type `damage`. Rise of Iron's primary, special
/// and heavy slots, 6, 7 and 8, give the ammunition whatever the base fires, so primary exotics
/// such as No Land Beyond, Universal Remote and Vex Mythoclast keep primary ammunition and
/// sidearms keep special. A Kinetic weapon goes in the Kinetic slot and an elemental one in the
/// Energy slot, as Shadowkeep sorts them, and a heavy weapon in the Power slot. A damage type the
/// source does not settle leaves the slot to the base.
fn slot_and_ammo(slot: u8, damage: Option<&str>) -> Result<(Option<&'static str>, &'static str)> {
    let inventory = damage.map(|d| if d == "kinetic" { "kinetic" } else { "energy" });
    Ok(match slot {
        6 => (inventory, "primary"),
        7 => (inventory, "special"),
        8 => (Some("power"), "heavy"),
        _ => anyhow::bail!("Unknown source weapon slot {slot}"),
    })
}

impl Source {
    /// Prepare a source weapon on the first native runtime with compatible bones and damage storage.
    /// The caller orders the bases, ordinary weapons of the source's type before exotics, so
    /// the base contributes firing and reloads rather than its own exotic behavior.
    pub fn prepare_weapon_on_base(
        &self,
        hash: u32,
        native: &Path,
        bases: &[u32],
        output: &Path,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<PathBuf> {
        let output = self.output(output)?;
        let output = crate::io::outside(
            &output,
            native.parent().context("Native installation root")?,
        )?;
        ensure!(
            !bases.is_empty(),
            "No native runtime of this weapon type is available"
        );
        // Select a compatible runtime before any conversion. A source-data or tool failure
        // cannot be repaired by rebuilding the same assets on another runtime.
        progress("Exporting source weapon…".into());
        let export = self.export_weapon(hash, &output.join("source"))?;
        let source: Value = serde_json::from_slice(&fs::read(export)?)?;
        let kinetic = perk::requires_kinetic(self, &source)?;
        let mut reader = Reader::discovery(native, &output.join("rig-checks"), false)?;
        let mut refused = Vec::new();
        for (index, &base) in bases.iter().enumerate() {
            crate::cancellation::check()?;
            progress(format!(
                "Checking native runtime {} of {}…",
                index + 1,
                bases.len()
            ));
            if kinetic {
                let tag = crate::tiger::shadowkeep::item_tag(&mut reader, base)?;
                let item = reader.tag(tag, Some(0x80807BEA))?;
                let sockets = item.array(item.pointer(0x68)?, 0x50, Some(0x808077C4))?;
                if sockets.iter().any(|&at| item.u16(at).ok() == Some(68)) {
                    let reason = format!(
                        "{base:08X}: the native type-68 damage socket cannot carry Kinetic damage"
                    );
                    progress(format!("Skipping native runtime {reason}"));
                    refused.push(reason);
                    continue;
                }
            }
            if let Err(error) = rig::weapon(self, &source["runtime"], base, &mut reader) {
                refused.push(format!("{base:08X}: {error:#}"));
                continue;
            }
            progress(format!("Using native runtime {base:08X}…"));
            fs::write(
                output.join("native-base-selection.json"),
                serde_json::to_vec_pretty(
                    &json!({"selected":base,"requires_kinetic":kinetic,"rejected":refused}),
                )?,
            )?;
            reader.finish()?;
            drop(reader);
            return self
                .prepare_exported_weapon(
                    hash,
                    native,
                    (base, bases),
                    &output,
                    &source,
                    options,
                    progress,
                )
                .with_context(|| format!("Source weapon {hash:08X}, native runtime {base:08X}"));
        }
        anyhow::bail!(
            "No native base of this type carries the source weapon:\n{}",
            refused.join("\n")
        )
    }

    /// Prepare a source weapon against an explicitly selected compatible native runtime.
    /// Matching skeletons establish the art binding, not source animation or firing parity.
    pub fn prepare_weapon(
        &self,
        hash: u32,
        native: &Path,
        donor: u32,
        output: &Path,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<PathBuf> {
        let output = self.output(output)?;
        let output = crate::io::outside(
            &output,
            native.parent().context("Native installation root")?,
        )?;
        let source_path = self.export_weapon(hash, &output.join("source"))?;
        let source: Value = serde_json::from_slice(&fs::read(source_path)?)?;
        self.prepare_exported_weapon(
            hash,
            native,
            (donor, &[]),
            &output,
            &source,
            options,
            progress,
        )
    }

    /// `donor` is the native runtime with the other native weapons of its type, which can lend
    /// it an ornament socket type.
    #[allow(clippy::too_many_arguments)]
    fn prepare_exported_weapon(
        &self,
        hash: u32,
        native: &Path,
        (donor, kin): (u32, &[u32]),
        output: &Path,
        source: &Value,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<PathBuf> {
        let weapon = self
            .weapons()?
            .into_iter()
            .find(|w| w.hash == hash)
            .context("Selected source weapon is missing")?;
        progress("Reading native weapon art…".into());
        let namespace = options
            .namespace
            .clone()
            .unwrap_or_else(|| crate::graph::import_namespace("d1_roi", &weapon.name, hash));
        let identity = crate::graph::import_identity(&namespace)?;
        let private_item = crate::graph::hash(&identity, "item_hash")?;
        let directory = output.join("graph");
        let mut assembly = Assembly::new(
            self,
            native,
            &directory,
            options,
            Some((&source["runtime"], donor)),
        )?;
        let donor_tag = crate::tiger::shadowkeep::item_tag(&mut assembly.native, donor)?;
        let definition = assembly.native.tag(donor_tag, Some(0x80807BEA))?;
        ensure!(
            definition.u8(0xB8)? <= 2 && definition.u8(0xBB)? == 1,
            "Selected native donor is not an instanced weapon"
        );
        let selectors = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
        let template =
            definition.u16(selectors.first().context("Native weapon art selector")? + 2)?;
        let mut parts = Vec::new();
        let dyes = source["dyes"].as_array().context("Source weapon dyes")?;
        let art = source["art"]["parts"]
            .as_array()
            .context("Source weapon parts")?;
        for (i, part) in art.iter().enumerate() {
            progress(format!("Converting model part {} of {}…", i + 1, art.len()));
            let symbol = assembly.body(part, dyes, &format!("part-{i}"))?;
            parts.push(json!({"source_assignment":part["assignment"],
                "key":key(&format!("parhelion/imported-gear/{private_item:08X}/art-{i}")),"parent":symbol}));
        }
        ensure!(!parts.is_empty(), "Source weapon has no converted art");
        let mut rows = source["art"]["rows"]
            .as_array()
            .context("Source weapon selectors")?
            .clone();
        native_rows(&mut rows, template)?;
        progress("Converting the item icon…".into());
        let icon = assembly.item_icon(weapon.strings)?;
        progress("Translating source stats…".into());
        let stats = stats::translate(&mut assembly, source)?;
        progress("Translating source firing inputs…".into());
        let mut firing = firing::prepare(&mut assembly, source, donor)?;
        let projectile_assets = firing["projectile"]
            .as_object_mut()
            .and_then(|report| report.remove("assets"))
            .filter(|assets| !assets.is_null());
        let mut firing_effect = firing["presentation"]
            .as_object_mut()
            .and_then(|report| report.remove("effect"))
            .filter(|effect| !effect.is_null());
        progress("Converting the source rig…".into());
        let mut native_rig = assembly.evidence[0]["native_runtime"].clone();
        if assembly.evidence[0]["source_owned"] == true {
            rig::prepare_owned(
                self,
                &source["runtime"],
                &mut assembly.native,
                &mut native_rig,
                &directory,
            )?;
        }
        // An unreadable marker set keeps the native runtime rows and says why.
        let runtime_markers =
            match marker::runtime_rows(&mut assembly, &source["runtime"], &native_rig) {
                Ok(report) => report,
                Err(error) => {
                    crate::cancellation::check()?;
                    json!({"rows":[],"limits":[format!("{error:#}")]})
                }
            };
        progress("Converting named audio…".into());
        let named_audio = crate::d1_roi::audio::prepare_named(
            self,
            &source["runtime"],
            &mut assembly.native,
            &mut native_rig,
            &directory,
        )
        .unwrap_or_else(|error| {
            json!({"status":"native_donor_only",
            "reason":format!("{error:#}"),"gameplay_verified":false})
        });
        progress("Converting weapon animation…".into());
        let mut runtime_animation = super::animation::prepare(
            self,
            &source["runtime"],
            &mut assembly.native,
            &native_rig,
            &directory,
        )
        .unwrap_or_else(|error| {
            json!({"status":"native","reason":format!("{error:#}"),"gameplay_verified":false})
          });
        progress("Converting first-person animation…".into());
        let mut first_person_animation = super::animation::prepare_first_person(
            self, &source["runtime"], &mut assembly.native, &native_rig, &directory,
        ).unwrap_or_else(|error| json!({"first_person_status":"native","reason":format!("{error:#}"),"gameplay_verified":false}));
        if assembly.evidence[0]["source_owned"] == true {
            rig::finish_owned(
                &mut assembly.native,
                &native_rig,
                &directory,
                &mut runtime_animation,
                &mut first_person_animation,
            )?;
        }
        progress("Converting third-person poses…".into());
        match super::animation::character::prepare(
            self,
            &source["runtime"],
            &mut assembly.native,
            &native_rig,
            &directory,
            key(&format!("d1/character/{private_item:08X}")),
        ) {
            Ok(character) => {
                first_person_animation["character"] = character;
                first_person_animation["character_status"] = json!("linked");
            }
            Err(error) => {
                first_person_animation["character_status"] = json!("native");
                first_person_animation["character_reason"] = json!(format!("{error:#}"));
            }
        }
        assembly.audio_assets = runtime_animation
            .as_object_mut()
            .and_then(|v| v.remove("audio_assets"));
        prepare_projectile_audio(&mut assembly, &mut firing, &mut firing_effect)?;
        progress("Translating source perks…".into());
        let (columns, variants, perks, damage) =
            perk::translate(&mut assembly, source, donor, perk::Host::Weapon)?;
        let limitations = weapon_limitations(
            &runtime_animation,
            &first_person_animation,
            &assembly.approximations,
            &firing,
        );
        // HUD artwork is presentation only, so a source without it still imports.
        let (hud, hud_png) = hud::prepare(self, source, &weapon.kind, &output.join("source"))
            .unwrap_or_else(|error| {
                (
                    json!({"status":"source_only","reason":format!("{error:#}")}),
                    None,
                )
            });
        let mut graph = json!({"kind":"weapon","source":"d1_roi","source_item":hash,"source_name":weapon.name,
            "source_icon_png":icon,"source_builds":self.report.builds,"source_stamp":self.report.stamp,
            "item_hash":private_item,"native_item":donor,"converter_revision":crate::graph::CONVERTER_REVISION,
            "nodes":assembly.nodes,"parent":parts[0]["parent"],"dyes":[],"dye_rows":[[],[],[]],
            "gear_art":{"selector_format":"shadowkeep","rows":rows,"parts":parts},
            "source_talents":source["talents"],"perk_translation":perks,"stat_translation":stats.report,"firing_translation":firing,
            "rendering":assembly.evidence,"equipment_animation":runtime_animation,"animation":first_person_animation,"audio":named_audio,
            "runtime_markers":runtime_markers,
            "collections_unconditional":true,"installable":true,"gameplay_verified":false,"limitations":limitations,
            "source_hud":hud});
        if let Some(audio) = assembly.audio_assets.take() {
            graph["audio_assets"] = audio;
        }
        if let Some(projectile) = projectile_assets {
            graph["projectile"] = projectile;
        }
        if let Some(effect) = firing_effect {
            graph["firing_effect"] = effect;
        }
        progress("Saving the weapon recipe…".into());
        crate::io::write_json(&directory.join("asset-graph.json"), &graph)?;
        crate::graph::check_particle_bindings(&directory)?;
        assembly.native.finish()?;
        self.unchanged()?;
        let reference = GraphReference::new(&directory, private_item)?;
        let rarity = match weapon.rarity {
            1 => "common",
            2 => "uncommon",
            3 => "rare",
            4 => "legendary",
            5 => "exotic",
            _ => anyhow::bail!("Unknown source weapon rarity"),
        };
        let mut recipe = json!({"schema":1,"namespace":namespace,"identity":identity,"name":weapon.name,
            "type_name":weapon.kind,"flavor":weapon.flavor,"collection_placement":"sunrise_badge",
            "donor":{"item_hash":format!("0x{donor:08X}")},"source":"Source: Rise of Iron",
            "overrides":{"rarity":rarity,"imported_graph":reference,"remove_lore":true,
                "base_sandbox_perks":[],"socket_columns":columns,"socket_plug_variants":variants,
                "investment_stats":stats.stats}});
        if let Some(damage) = damage {
            recipe["overrides"]["modern_damage_type"] = json!(damage);
        }
        let (inventory, ammo) = slot_and_ammo(weapon.slot, damage)?;
        if let Some(inventory) = inventory {
            recipe["overrides"]["inventory_slot"] = json!(inventory);
        }
        recipe["overrides"]["ammo_type"] = json!(ammo);
        if let Some(group) = stats.group {
            recipe["overrides"]["custom_stat_group"] = group;
        }
        if let Some(png) = hud_png {
            crate::hud_icon::apply(&mut recipe, &png);
        }
        crate::artwork::apply(&mut recipe, &directory)?;
        let path = output.join("weapon.parhelion.json");
        crate::io::write_json(&path, &recipe)?;
        let ornaments = if options.ornaments {
            self.import_weapon_ornaments(
                &ornament::Owner {
                    weapon: &weapon,
                    source,
                    native,
                    donor,
                    kin,
                    namespace: &namespace,
                    item: private_item,
                    template,
                    recipe: &path,
                    output: &output.join("ornaments"),
                },
                options,
                progress,
            )?
        } else {
            json!({"skipped":"Ornaments were not imported"})
        };
        crate::io::write_json(&output.join("ornaments.json"), &ornaments)?;
        Ok(path)
    }
}

fn prepare_projectile_audio(
    assembly: &mut Assembly<'_>,
    firing: &mut Value,
    firing_effect: &mut Option<Value>,
) -> Result<()> {
    // A translated projectile spawn's cues join the same closure, which talents then extend.
    if !assembly.projectile_cues.is_empty() {
        let mut cues = assembly
            .audio_assets
            .as_ref()
            .map(|audio| serde_json::from_value::<BTreeSet<u32>>(audio["cues"].clone()))
            .transpose()
            .context("Prepared clip audio cue set")?
            .unwrap_or_default();
        cues.extend(assembly.projectile_cues.iter().copied());
        let plan = match crate::d1_roi::audio::plan(assembly.source, &mut assembly.native, &cues) {
            Ok(plan) => plan,
            // The translated firing presentation's cues must not cost the weapon the rest of
            // its audio. Without them its flows cannot sound, so the base's presentation stays.
            Err(error) if firing_effect.is_some() => {
                crate::cancellation::check()?;
                let added =
                    serde_json::from_value::<Vec<u32>>(firing["presentation"]["cues"].clone())
                        .context("Translated firing presentation cue list")?;
                for cue in &added {
                    cues.remove(cue);
                    assembly.projectile_cues.remove(cue);
                }
                *firing_effect = None;
                firing["presentation"] = json!({"status":"native",
                    "reason":format!("The weapon's audio closure refused the firing cues: {error:#}")});
                crate::d1_roi::audio::plan(assembly.source, &mut assembly.native, &cues)?
            }
            Err(error) => return Err(error),
        };
        let mut audio = plan.emit(&assembly.folder.join("audio-assets"))?;
        for node in audio["nodes"]
            .as_array_mut()
            .context("Projectile spawn audio nodes")?
        {
            node["file"] = json!(format!(
                "audio-assets/{}",
                node["file"]
                    .as_str()
                    .context("Projectile spawn audio file")?
            ));
        }
        audio["installable"] = json!(true);
        assembly.audio_assets = Some(audio);
    }
    Ok(())
}

fn weapon_limitations(
    runtime_animation: &Value,
    first_person_animation: &Value,
    approximations: &BTreeSet<String>,
    firing: &Value,
) -> Vec<String> {
    let animation_limit = match (
        runtime_animation["status"] == "linked",
        first_person_animation["first_person_status"] == "linked",
    ) {
        (true, true) => {
            "Supported weapon and named first-person actions use converted source motion. Other actions and firing effects retain the selected native runtime."
        }
        (true, false) => {
            "Supported weapon clips use converted source motion. First-person actions retain the selected native runtime."
        }
        (false, true) => {
            "Supported named first-person actions use converted source motion. Equipment clips and other actions retain the selected native runtime."
        }
        (false, false) => "Firing and reload animation use the selected native runtime.",
    };
    let mut limitations = [
        animation_limit,
        "Base stats use source investment values. Qualified firing curves retain source timing and charge inputs. Qualified projectile counterparts retain native behavior with source response operands. Unsupported firing graphs and other stat translation use the selected native donor.",
        "Matched named audio uses source banks and media with the native event's trigger and acoustic settings. Other audio uses the selected native runtime. Supported sight lights retain source transforms and live parameters on native shaders. Other auxiliary sight effects require separate translation.",
        "Unsupported talent choices, scaled source stat adjustments and talent progression are not translated.",
    ]
    .map(String::from)
    .to_vec();
    limitations.push(if first_person_animation["character_status"] == "linked" {
        "Qualified standing and crouched poses retain source body motion, holding and finger layers. Dynamic aim, movement and combat actions retain native behavior.".into()
    } else {
        format!("Third-person poses retain native behavior: {}", first_person_animation["character_reason"].as_str().unwrap_or("source holding profile was not converted"))
    });
    limitations.extend(approximations.iter().cloned());
    if firing["projectile"]["status"] == "retained_native" {
        limitations.extend(
            firing["projectile"]["compatibility"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from),
        );
    }
    limitations
}
