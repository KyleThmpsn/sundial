//! Source weapon art on a native runtime with identical root and attachment rigs.
use super::*;
use crate::GraphReference;
mod perk;
mod stats;

fn key(text: &str) -> u32 {
    let value = text.bytes().fold(0x811C9DC5u32, |h, b| {
        h.wrapping_mul(16777619) ^ u32::from(b)
    });
    if matches!(value, 0 | u32::MAX | 0x811C9DC5) {
        value ^ 0x10000
    } else {
        value
    }
}

impl Source {
    /// Prepare a source weapon on the first native runtime in `bases` whose rigs match it.
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
        // Check each base's rigs against one export before converting on it, since the rigs
        // decide most refusals and a conversion reads the whole source again.
        let export = self.export_weapon(hash, &output.join("source"))?;
        let source: Value = serde_json::from_slice(&fs::read(export)?)?;
        let mut reader = Reader::discovery(native, &output.join("rig-checks"), false)?;
        let mut refused = Vec::new();
        for &base in bases {
            crate::cancellation::check()?;
            if let Err(error) = rig::weapon(&source["runtime"], base, &mut reader) {
                refused.push(format!("{base:08X}: {error:#}"));
                continue;
            }
            progress(format!("Building on native base {base:08X}"));
            match self.prepare_weapon(
                hash,
                native,
                base,
                &output.join(format!("{base:08X}")),
                options,
                progress,
            ) {
                Ok(recipe) => return Ok(recipe),
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(error) => {
                    progress(format!("Native base {base:08X} refused: {error:#}"));
                    refused.push(format!("{base:08X}: {error:#}"));
                }
            }
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
        let weapon = self
            .weapons()?
            .into_iter()
            .find(|w| w.hash == hash)
            .context("Selected source weapon is missing")?;
        progress(format!("Reading {}", weapon.name));
        let source_path = self.export_weapon(hash, &output.join("source"))?;
        let source: Value = serde_json::from_slice(&fs::read(source_path)?)?;
        let namespace = format!("parhelion.import.d1_roi.{hash:08x}");
        let identity = crate::d2_mot::batch::identity(&namespace)?;
        let private_item = crate::graph::hash(&identity, "item_hash")?;
        let directory = output.join("graph");
        let mut assembly = Assembly::new(
            self,
            native,
            &directory,
            options,
            Some((&source["runtime"], donor)),
        )?;
        let donor_tag = crate::d2_mot::assets::item::find_native(&mut assembly.native, donor)?;
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
        for (i, part) in source["art"]["parts"]
            .as_array()
            .context("Source weapon parts")?
            .iter()
            .enumerate()
        {
            progress(format!("Converting {} part {}", weapon.name, i + 1));
            let symbol = assembly.body(part, dyes, &format!("part-{i}"))?;
            parts.push(json!({"source_assignment":part["assignment"],
                "key":key(&format!("parhelion/imported-gear/{private_item:08X}/art-{i}")),"parent":symbol}));
        }
        ensure!(!parts.is_empty(), "Source weapon has no converted art");
        let mut rows = source["art"]["rows"]
            .as_array()
            .context("Source weapon selectors")?
            .clone();
        for row in &mut rows {
            row["template_index"] = json!(template);
        }
        let icon = assembly.item_icon(weapon.strings)?;
        let (columns, variants, perks, damage) = perk::translate(&mut assembly, &source, donor)?;
        let stats = stats::translate(&mut assembly, &source)?;
        progress(format!("Converting {} runtime animation", weapon.name));
        let native_rig = assembly.evidence[0]["native_runtime"].clone();
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
        let audio_assets = runtime_animation
            .as_object_mut()
            .and_then(|v| v.remove("audio_assets"));
        let animation_limit = if runtime_animation["status"] == "linked" {
            "Supported weapon clips use source motion and direct audio cues. First-person arm animation and firing effects use the selected native runtime. Event dispatch and playback require in-game acceptance."
        } else {
            "Firing, reload animation and audio use the selected native runtime."
        };
        let mut limitations = [
            animation_limit,
            "Base stats are the source investment values. The native runtime's stat translation decides their gameplay effect, which requires in-game acceptance.",
            "Auxiliary sight effects and audio outside translated clip events use the selected native runtime.",
            "Unsupported talent choices, scaled source stat adjustments and talent progression are not translated.",
            "Rendering, aiming, repeated firing and selection cleanup require in-game acceptance.",
        ]
        .map(String::from)
        .to_vec();
        limitations.extend(assembly.approximations.iter().cloned());
        let mut graph = json!({"kind":"weapon","source":"d1_roi","source_item":hash,"source_name":weapon.name,
            "source_icon_png":icon,"source_builds":self.report.builds,"source_stamp":self.report.stamp,
            "item_hash":private_item,"native_item":donor,"converter_revision":crate::graph::CONVERTER_REVISION,
            "nodes":assembly.nodes,"parent":parts[0]["parent"],"dyes":[],"dye_rows":[[],[],[]],
            "gear_art":{"selector_format":"shadowkeep","rows":rows,"parts":parts},
            "source_talents":source["talents"],"perk_translation":perks,"stat_translation":stats.report,
            "rendering":assembly.evidence,"equipment_animation":runtime_animation,
            "collections_unconditional":true,"installable":true,"gameplay_verified":false,"limitations":limitations});
        if let Some(audio) = audio_assets {
            graph["audio_assets"] = audio;
        }
        crate::io::write_json(&directory.join("asset-graph.json"), &graph)?;
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
        if let Some(group) = stats.group {
            recipe["overrides"]["custom_stat_group"] = group;
        }
        crate::artwork::apply(&mut recipe, &directory)?;
        let path = output.join("weapon.parhelion.json");
        crate::io::write_json(&path, &recipe)?;
        Ok(path)
    }
}
