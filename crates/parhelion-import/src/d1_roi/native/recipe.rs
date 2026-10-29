//! Prepare native armor and its linked private appearance choices as one recipe.
use super::*;
use crate::GraphReference;

fn hash(text: &str) -> u32 {
    text.bytes().fold(0x811C9DC5u32, |h, b| {
        h.wrapping_mul(16777619) ^ u32::from(b)
    })
}

fn number(value: &Value) -> Result<u32> {
    u32::try_from(value.as_u64().context("Armor identity missing")?).map_err(Into::into)
}

fn bucket(slot: u8) -> Result<u32> {
    match slot {
        1 => Ok(3448274439),
        2 => Ok(3551918588),
        3 => Ok(14239492),
        4 => Ok(20886954),
        5 => Ok(1585787867),
        _ => anyhow::bail!("Unknown source armor slot"),
    }
}

impl Assembly<'_> {
    #[allow(clippy::too_many_arguments)]
    fn armor_graph(
        &mut self,
        item: &Armor,
        identity: u32,
        donor: u32,
        template_index: u16,
        bodies: &[Value],
        dyes: &[Value],
        ornaments: &[Value],
    ) -> Result<GraphReference> {
        ensure!(
            bodies.len() == 2
                && bodies[0]["body"] == "masculine"
                && bodies[1]["body"] == "feminine",
            "Armor body selector order differs"
        );
        let mut parts = Vec::new();
        let mut singles = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let symbol = self.body(body, dyes, &format!("body-{index}"))?;
            let assignment = number(&body["assignment"])?;
            parts.push(json!({"source_assignment":assignment,"key":hash(&format!("parhelion/imported-gear/{identity:08X}/art-{index}")),"parent":symbol}));
            singles.push(assignment);
        }
        let icon = self.icon(item)?;
        let (native_dyes, dye_rows) = self.armor_dyes(dyes, donor, identity)?;
        let mut limitations = vec![
            "Source effect sequences and their particle events are not translated.".to_owned(),
        ];
        limitations.extend(self.approximations.iter().cloned());
        let mut graph = json!({"kind":"armor","source":"d1_roi","source_item":item.hash,"source_name":item.name,"source_icon_png":icon,
            "collection_set":item.collection_set,
            "source_class":item.class_type,"source_bucket":bucket(item.slot)?,"source_rarity":item.rarity,
            "item_hash":identity,"native_item":donor,"converter_revision":crate::graph::CONVERTER_REVISION,
            "source_builds":self.source.report.builds,"source_stamp":self.source.report.stamp,"source_ornaments":ornaments,
            "nodes":self.nodes,"parent":parts[0]["parent"],"dyes":native_dyes,"dye_rows":[[],dye_rows,[]],
            "gear_art":{"selector_format":"shadowkeep","rows":[{"class":item.class_type,"flags":0,"template_index":template_index,"singles":singles,"slots":[]}],"parts":parts},
            "collections_unconditional":true,"rendering":self.evidence,"installable":true,"gameplay_verified":false,
            "limitations":limitations});
        if let Some(audio) = &self.audio_assets {
            graph["audio_assets"] = audio.clone();
        }
        crate::io::write_json(&self.folder.join("asset-graph.json"), &graph)?;
        self.native.finish()?;
        GraphReference::new(&self.folder, identity)
    }
}

impl Source {
    /// Convert both source bodies and linked ornaments into a portable native recipe.
    pub fn prepare_armor(
        &self,
        item: u32,
        native: &Path,
        donors: &Value,
        output: &Path,
        options: &NativeOptions,
        progress: &mut dyn FnMut(String),
    ) -> Result<PathBuf> {
        let output = self.output(output)?;
        let output = crate::io::outside(
            &output,
            native.parent().context("Native installation root")?,
        )?;
        let armor = self
            .armor()?
            .into_iter()
            .find(|a| a.hash == item)
            .context("Selected source armor is missing")?;
        let mut assets = BTreeMap::new();
        progress("Reading source armor art…".into());
        let bodies = self.armor_bodies(&armor, &mut assets)?;
        let ornaments = if options.ornaments {
            progress("Reading source ornaments…".into());
            self.ornaments(&armor, &mut assets)?
        } else {
            Vec::new()
        };
        progress("Reading source dyes and talents…".into());
        let dyes = self.dyes(armor.definition, &mut assets)?;
        let talents = self.item_talents(armor.definition, &mut assets)?;
        let source = json!({"source":"d1_roi","item":armor,"talents":talents,
            "builds":self.report.builds,"package_stamp":self.report.stamp});
        fs::create_dir_all(output.join("source"))?;
        crate::io::write_json(&output.join("source/armor.json"), &source)?;
        let namespace = options
            .namespace
            .clone()
            .unwrap_or_else(|| crate::graph::import_namespace("d1_roi", &armor.name, item));
        let identity = crate::graph::import_identity(&namespace)?;
        let private_item = u32::from_str_radix(
            identity["item_hash"]
                .as_str()
                .context("Native identity")?
                .trim_start_matches("0x"),
            16,
        )?;
        let mut assembly = Assembly::new(self, native, &output.join("graph"), options, None)?;
        progress("Finding a native armor template…".into());
        let mut selected = None;
        for donor in donors["gear"]
            .as_array()
            .context("Native gear donors are missing")?
        {
            if donor["bucket_hash"].as_u64() != Some(u64::from(bucket(armor.slot)?))
                || donor["class_type"].as_u64() != Some(u64::from(armor.class_type))
                || donor["collection_backed"] != true
            {
                continue;
            }
            let hash = number(&donor["hash"])?;
            let tag = crate::tiger::shadowkeep::item_tag(&mut assembly.native, hash)?;
            let definition = assembly.native.tag(tag, Some(0x80807BEA))?;
            if definition.u8(0xBA)? == 5 {
                continue;
            }
            let selectors = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
            let Some(selector) = selectors.first() else {
                continue;
            };
            let sockets = crate::tiger::shadowkeep::sockets(&mut assembly.native, hash)?;
            let socket = sockets["sockets"]
                .as_array()
                .context("Native socket report")?
                .iter()
                .find(|s| {
                    s["choices"].as_array().is_some_and(|choices| {
                        choices.iter().any(|c| {
                            c["default"] == true
                                && c["item_kind"] == 13
                                && c["art_indices"].as_array().is_some_and(Vec::is_empty)
                        }) && choices
                            .iter()
                            .filter(|c| {
                                c["default"] != true
                                    && c["item_kind"] == 13
                                    && c["art_indices"].as_array().is_some_and(|a| !a.is_empty())
                            })
                            .count()
                            >= ornaments.len()
                    })
                })
                .cloned();
            if !ornaments.is_empty() && socket.is_none() {
                continue;
            }
            selected = Some((donor.clone(), definition.u16(selector + 2)?, socket));
            break;
        }
        let (donor, template, socket) = selected.context(
            "No native armor template has the required class, slot and appearance choices",
        )?;
        let donor_hash = number(&donor["hash"])?;
        progress("Translating source talents…".into());
        let (columns, variants, perks, damage) =
            perk::translate(&mut assembly, &source, donor_hash, perk::Host::Armor)?;
        ensure!(
            damage.is_none(),
            "Armor talents cannot set the item's damage type"
        );
        let mut columns = columns.as_array().context("Armor talent columns")?.clone();
        let mut variants = variants
            .as_array()
            .context("Armor talent variants")?
            .clone();
        progress("Converting the armor model…".into());
        let mut reference = assembly.armor_graph(
            &armor,
            private_item,
            donor_hash,
            template,
            &bodies,
            &dyes,
            &ornaments,
        )?;
        if let Some(socket) = socket.filter(|_| !ornaments.is_empty()) {
            let socket_index =
                usize::try_from(socket["index"].as_u64().context("Ornament socket index")?)?;
            let candidates = socket["choices"]
                .as_array()
                .context("Native appearance choices")?;
            let reset = candidates
                .iter()
                .find(|c| c["default"] == true)
                .context("Native appearance reset missing")?;
            let reset = number(&reset["hash"])?;
            let candidates = candidates
                .iter()
                .filter(|c| {
                    c["default"] != true
                        && c["item_kind"] == 13
                        && c["art_indices"].as_array().is_some_and(|a| !a.is_empty())
                })
                .collect::<Vec<_>>();
            let mut choices = vec![json!(format!("0x{reset:08X}"))];
            for (index, ornament) in ornaments.iter().enumerate() {
                progress(format!(
                    "Converting ornament {} of {}…",
                    index + 1,
                    ornaments.len()
                ));
                let info = &ornament["item"];
                let mut source_item = Armor {
                    hash: number(&info["hash"])?,
                    name: info["name"].as_str().context("Ornament name")?.to_owned(),
                    kind: armor.kind.clone(),
                    class_type: armor.class_type,
                    slot: armor.slot,
                    rarity: armor.rarity,
                    collection_set: None,
                    definition: number(&info["definition"])?,
                    strings: number(&info["strings"])?,
                    arrangements: Vec::new(),
                };
                let choice = index + 1;
                let plug = number(&candidates[index]["hash"])?;
                let target = hash(&format!(
                    "parhelion/{namespace}/socket/{socket_index}/choice/{choice}/private-plug"
                ));
                let mut ornament_assembly = Assembly::new(
                    self,
                    native,
                    &output
                        .join(crate::graph::ORNAMENTS)
                        .join(format!("{:08X}", source_item.hash))
                        .join(crate::graph::GRAPH),
                    options,
                    None,
                )?;
                let (name, icon_container) =
                    ornament_assembly.ornament_display(armor.definition, ornament)?;
                if !name.is_empty()
                    && super::super::localization::Labels::new(self)?
                        .text(self, &self.read(source_item.strings)?, 0x78)?
                        .is_empty()
                {
                    source_item.name = name;
                }
                // An appearance item may override only some of its parent's
                // dye slots. Retain the remaining slots and layer precedence.
                let mut ornament_dyes = dyes.clone();
                ornament_dyes.extend(self.dyes(source_item.definition, &mut assets)?);
                ornament_dyes.sort_by_key(|dye| match dye["layer"].as_str() {
                    Some("default") => 0,
                    Some("custom") => 1,
                    Some("fixed") => 2,
                    _ => unreachable!("Source::dyes declares the layer names"),
                });
                let graph = ornament_assembly.armor_graph(
                    &source_item,
                    target,
                    donor_hash,
                    template,
                    ornament["bodies"]
                        .as_array()
                        .context("Ornament body selectors")?,
                    &ornament_dyes,
                    &[],
                )?;
                let manifest = graph.directory.join("asset-graph.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
                let icon = if let Some(icon) = value["source_icon_png"].as_str() {
                    Some(icon.to_owned())
                } else {
                    ornament_assembly.container_icon(icon_container)?
                };
                value["ornament_icon_png"] = json!(icon);
                value["rendering"] = json!(ornament_assembly.evidence);
                value["ornament"] = json!({"target_item":private_item,"target_socket":socket_index,"choice_index":choice,"native_plug":plug,"default_reset":reset,"name":source_item.name});
                crate::io::write_json(&manifest, &value)?;
                reference
                    .attachments
                    .push(GraphReference::new(&graph.directory, private_item)?);
                choices.push(json!(format!("0x{plug:08X}")));
                variants.push(json!({"socket_index":socket_index,"choice_index":choice,"source_plug_hash":format!("0x{plug:08X}"),"name":source_item.name,"description":"Changes this armor's appearance. Select the default ornament to restore its original appearance.","replace_effects":true,"sandbox_perks":[]}));
            }
            ensure!(
                socket_index < columns.len(),
                "Ornament socket exceeds the armor columns"
            );
            ensure!(
                columns[socket_index].is_null(),
                "Armor perk layout overlaps its ornament socket"
            );
            columns[socket_index] = json!({"choices":choices,"socket_type":socket["socket_type"],"choice_conditions":[],"choice_weight_bits":[],"randomized_selection_program":[]});
        }
        progress("Saving the armor recipe…".into());
        let manifest = reference.directory.join("asset-graph.json");
        let mut graph: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
        graph["source_talents"] = talents;
        graph["perk_translation"] = perks;
        graph["limitations"].as_array_mut().context("Armor limitations")?.extend([
            json!("Supported armor talents use private native effects."),
            json!("Native base stats remain the baseline. Source armor stat tiers, ability grants, progression and unsupported talent effects are not translated."),
        ]);
        crate::io::write_json(&manifest, &graph)?;
        let attachments = std::mem::take(&mut reference.attachments);
        reference = GraphReference::new(&reference.directory, private_item)?;
        reference.attachments = attachments;
        reference.validate_reusable("armor")?;
        self.unchanged()?;
        let rarity = match armor.rarity {
            1 => "common",
            2 => "uncommon",
            3 => "rare",
            4 => "legendary",
            5 => "exotic",
            _ => anyhow::bail!("Unknown source armor rarity"),
        };
        let flavor = super::super::localization::Labels::new(self)?.text(
            self,
            &self.read(armor.strings)?,
            0x88,
        )?;
        let mut recipe = json!({"schema":1,"kind":"armor","collection_placement":"sunrise_badge","namespace":namespace,"identity":identity,"name":armor.name,"type_name":armor.kind,"flavor":flavor,
            "donor":{"item_hash":format!("0x{donor_hash:08X}"),"expected_name":donor["name"]},"source":"Source: Rise of Iron",
            "overrides":{"rarity":rarity,"imported_graph":reference,"base_sandbox_perks":[],"socket_columns":columns,"socket_plug_variants":variants,"remove_lore":true}});
        crate::artwork::apply(&mut recipe, &output.join("graph"))?;
        let path = output.join("armor.parhelion.json");
        crate::io::write_json(&path, &recipe)?;
        Ok(path)
    }
}
