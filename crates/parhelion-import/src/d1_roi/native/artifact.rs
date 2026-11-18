//! Equipment-only artifacts retain source talents without borrowing a weapon or armor model.
use super::*;
use crate::GraphReference;

fn donor(native: &mut Reader) -> Result<u32> {
    let globals = native
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|tag| tag.name == "investment_globals")
        .context("Native globals")?
        .hash
        .0;
    let globals = native.tag(globals, None)?;
    let root = native.tag(globals.u32(16)?, None)?;
    let items = native.tag(root.u32(8 + 48 * 16)?, None)?;
    let mut scan = native.shared_discovery();
    for row in items.array(8, 24, Some(0x80807BE8))? {
        let definition = scan.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        if definition.u8(0xB8)? != 49
            || definition.u64(0x10)? == 0
            || definition.u64(0x88)? != 0
            || definition.u64(0x70)? == 0
        {
            continue;
        }
        let equipment = definition.pointer(0x10)?;
        if definition.u32(equipment - 4)? == 0x80807C02
            && definition.u16(equipment + 0x18)? == 0x0112
            && definition.u16(equipment + 0x1A)? == u16::MAX
        {
            return items.u32(row);
        }
    }
    anyhow::bail!("No native artifact equipment template is available")
}

impl Source {
    /// Prepare an unrestricted source artifact and its qualified equipped talents.
    pub fn prepare_artifact(
        &self,
        item: u32,
        native: &Path,
        output: &Path,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<PathBuf> {
        let artifact = self
            .artifacts()?
            .into_iter()
            .find(|a| a.hash == item)
            .context("Selected source artifact is missing")?;
        // The native artifact template is unrestricted, so a source class travels in the
        // recipe and becomes the item's own equip condition.
        let class = match artifact.class_type {
            0 => Some("Titan"),
            1 => Some("Hunter"),
            2 => Some("Warlock"),
            3 => None,
            _ => anyhow::bail!("Unknown source artifact class"),
        };
        let output = self.output(output)?;
        let output = crate::io::outside(
            &output,
            native.parent().context("Native installation root")?,
        )?;
        progress("Reading source artifact talents…".into());
        let talents = self.item_talents(artifact.definition, &mut BTreeMap::new())?;
        let source = json!({"source":"d1_roi","item":artifact,"talents":talents,
            "builds":self.report.builds,"package_stamp":self.report.stamp});
        fs::create_dir_all(output.join("source"))?;
        crate::io::write_json(&output.join("source/artifact.json"), &source)?;
        let namespace = options
            .namespace
            .clone()
            .unwrap_or_else(|| crate::graph::import_namespace("d1_roi", &artifact.name, item));
        let identity = crate::graph::import_identity(&namespace)?;
        let private_item = u32::from_str_radix(
            identity["item_hash"]
                .as_str()
                .context("Native identity")?
                .trim_start_matches("0x"),
            16,
        )?;
        let folder = output.join("graph");
        let mut assembly = Assembly::new(self, native, &folder, options, None)?;
        let donor = donor(&mut assembly.native)?;
        progress("Translating source artifact talents…".into());
        let (columns, variants, perks, damage) =
            perk::translate(&mut assembly, &source, donor, perk::Host::Artifact)?;
        ensure!(
            damage.is_none(),
            "Artifact talents cannot set an item damage type"
        );
        ensure!(
            assembly.nodes.is_empty() && assembly.human.is_none(),
            "Artifact talents require an unsupported model or character action"
        );
        let icon = assembly.item_icon(artifact.strings)?;
        let mut graph = json!({"kind":"artifact","source":"d1_roi","source_item":item,
            "source_name":artifact.name,"source_icon_png":icon,"source_class":artifact.class_type,
            "source_bucket":1_506_418_338u32,"source_rarity":artifact.rarity,
            "item_hash":private_item,"native_item":donor,"converter_revision":crate::graph::CONVERTER_REVISION,
            "source_builds":self.report.builds,"source_stamp":self.report.stamp,
            "nodes":[],"source_talents":talents,"perk_translation":perks,
            "collections_unconditional":true,"collections_no_cost":true,
            "installable":true,"gameplay_verified":false,
            "limitations":["Supported talents use private equipped native effects. Unsupported talents are listed individually.",
                "Source random stat rolls and progression are not translated.",
                "A source class restriction becomes a native class equip condition.",
                "Equip activation, selection changes, repeated use and cleanup need an in-game check."]});
        if let Some(audio) = assembly.audio_assets.take() {
            graph["audio_assets"] = audio;
        }
        // The source strings name a second image at +70, which the source inspection screen
        // draws behind the artifact. It travels beside the icon as the icon's large picture.
        let backdrop = self.icon_png(self.read(artifact.strings)?.u32(0x70)?, None)?;
        if let Some(png) = &backdrop {
            let file = "source-backdrop.png";
            fs::write(folder.join(file), png)?;
            graph["source_backdrop_png"] = json!(file);
        }
        if icon.is_none() {
            graph["limitations"]
                .as_array_mut()
                .context("Artifact limitations")?
                .push(json!(
                    "The source icon is unavailable. The native artifact icon is retained."
                ));
        }
        assembly.native.finish()?;
        crate::io::write_json(&folder.join("asset-graph.json"), &graph)?;
        let reference = GraphReference::new(&folder, private_item)?;
        reference.validate_reusable("artifact")?;
        let rarity = match artifact.rarity {
            1 => "common",
            2 => "uncommon",
            3 => "rare",
            4 => "legendary",
            5 => "exotic",
            _ => anyhow::bail!("Unknown source artifact rarity"),
        };
        let flavor = super::super::localization::Labels::new(self)?.text(
            self,
            &self.read(artifact.strings)?,
            0x88,
        )?;
        let mut recipe = json!({"schema":1,"kind":"artifact","collection_placement":"sunrise_badge",
            "namespace":namespace,"identity":identity,"name":artifact.name,"type_name":artifact.kind,
            "flavor":flavor,"source":"Source: Rise of Iron","donor":{"item_hash":format!("0x{donor:08X}")},
            "overrides":{"rarity":rarity,"imported_graph":reference,"base_sandbox_perks":[],
                "socket_columns":columns,"socket_plug_variants":variants,"remove_lore":true}});
        if let Some(class) = class {
            recipe["overrides"]["armor_class"] = json!(class);
        }
        crate::artwork::apply(&mut recipe, &folder)?;
        if let Some(png) = backdrop
            && recipe["overrides"]["icon_edit"]["imported_image"].is_object()
        {
            use base64::{Engine, engine::general_purpose::STANDARD};
            recipe["overrides"]["icon_edit"]["backdrop_image"] =
                json!({"png_base64":STANDARD.encode(png)});
        }
        self.unchanged()?;
        let path = output.join("artifact.parhelion.json");
        crate::io::write_json(&path, &recipe)?;
        Ok(path)
    }
}
