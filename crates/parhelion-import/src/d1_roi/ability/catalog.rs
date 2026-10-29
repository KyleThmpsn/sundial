//! Complete source subclass choices, separate from readiness to emit native behavior.
use super::*;
mod definition;
pub use definition::AbilityCatalog;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubclassCatalog {
    pub item: u32,
    pub name: String,
    pub description: String,
    pub grid: u16,
    pub ability_row: u8,
    pub pattern: u32,
    pub graph: u32,
    pub controller: u32,
    #[serde(default)]
    pub controller_class: u32,
    pub bank: u32,
    #[serde(default)]
    pub abilities: Vec<AbilityCatalog>,
    pub nodes: Vec<SubclassNode>,
    pub source_inputs: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubclassNode {
    pub node: u32,
    pub group_key: u32,
    pub node_flags: u32,
    pub row: i16,
    pub column: i16,
    pub record: String,
    pub steps: Vec<SubclassStep>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubclassStep {
    pub step: u32,
    pub name: String,
    pub description: String,
    /// Automatic contributions and required singleton groups in a fully unlocked build.
    /// This does not describe a new character's initial visible selections.
    pub foundation: Option<bool>,
    /// All decoded variants, including grants, perks, stats and appearance choices.
    pub source: Value,
}

impl Source {
    /// Read every ability definition and its declared execution records, including unsupported families.
    /// This source census does not certify native graph or transitive resource conversion.
    pub fn ability_catalog(&self) -> Result<Vec<AbilityCatalog>> {
        ensure!(
            self.report
                .selected
                .iter()
                .all(|package| package.platform == 8),
            "Ability discovery currently supports Xbox One Rise of Iron packages"
        );
        let abilities = definition::read(self, &mut BTreeMap::new())?;
        self.unchanged()?;
        Ok(abilities
            .into_values()
            .map(|ability| ability.catalog)
            .collect())
    }

    /// Preserve every source subclass grid and all of its referenced ability families.
    /// A catalog is an intake record and does not certify native materialization.
    pub fn subclass_choices(&self) -> Result<Vec<SubclassCatalog>> {
        ensure!(
            self.report
                .selected
                .iter()
                .all(|package| package.platform == 8),
            "Subclass discovery currently supports Xbox One Rise of Iron packages"
        );
        let mut common = BTreeMap::new();
        let abilities = definition::read(self, &mut common)?;
        let (definitions_tag, definitions) = self.table(0x80801933)?;
        let (strings_tag, strings) = self.table(0x808034C7)?;
        let definition_rows = definitions.array(8, 24, Some(0x808016BD))?;
        let string_rows = strings.array(8, 16, Some(0x808033E8))?;
        ensure!(
            !definition_rows.is_empty() && definition_rows.len() == string_rows.len(),
            "Source subclass item and string ordinals disagree"
        );
        common.insert(definitions_tag, definitions.clone());
        common.insert(strings_tag, strings.clone());
        let mut labels = super::super::localization::Labels::new(self)?;
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for (row, text_row) in definition_rows.into_iter().zip(string_rows) {
            let definition_tag = definitions.u32(row + 16)?;
            let definition = self.read(definition_tag)?;
            let item = definitions.u32(row)?;
            ensure!(
                self.reference(definition_tag)? == 0x80801806 && definition.u32(0x78)? == item,
                "Source subclass item identity differs"
            );
            if definition.u64(0x18)? == 0 {
                continue;
            }
            let client = definition.pointer(0x18)?;
            ensure!(
                definition.u32(client.checked_sub(4).context("Source item client marker")?)?
                    == 0x80801706,
                "Source subclass client type differs"
            );
            if definition.u8(client + 0x18)? != 0 {
                continue;
            }
            ensure!(seen.insert(item), "Duplicate source subclass identity");
            let mut inputs = common.clone();
            let talents = self.item_talents(definition_tag, &mut inputs)?;
            let source_nodes = talents["nodes"]
                .as_array()
                .context("Source subclass nodes")?;
            ensure!(!source_nodes.is_empty(), "Source subclass grid is empty");
            let mut selected_rows = BTreeSet::new();
            let mut equipped_supers = BTreeSet::new();
            for node in source_nodes {
                for step in node["steps"].as_array().context("Source subclass steps")? {
                    for variant in step["variants"]
                        .as_array()
                        .context("Source subclass variants")?
                    {
                        for grant in grants(variant)? {
                            if grant.u8(11)? != u8::MAX && grant.u8(12)? == 1 {
                                equipped_supers.insert(grant.u8(11)?);
                            }
                            for row in [grant.u8(4)?, grant.u8(11)?] {
                                if row != u8::MAX {
                                    ensure!(
                                        abilities.contains_key(&row),
                                        "Source subclass grant exceeds the ability table"
                                    );
                                    selected_rows.insert(row);
                                }
                            }
                        }
                    }
                }
            }
            let equipped_supers = equipped_supers.into_iter().collect::<Vec<_>>();
            let [ability_row] = equipped_supers.as_slice() else {
                anyhow::bail!("Source subclass {item:08X} has no unique equipped Super grant");
            };
            let ability = &abilities[ability_row];
            let primary = &ability.catalog;
            let controller = primary.controller;
            let bank = primary.bank;
            for selected in &selected_rows {
                inputs.extend(abilities[selected].inputs.clone());
            }
            let shared_tag = number(&talents, "shared_grid")?;
            let shared = inputs
                .get(&shared_tag)
                .context("Source subclass shared grid")?;
            let foundations = foundations(shared, source_nodes)?;
            let mut nodes = Vec::new();
            let mut node_indices = BTreeSet::new();
            for node in source_nodes {
                let index = number(node, "index")?;
                ensure!(node_indices.insert(index), "Duplicate source subclass node");
                let offset = usize::try_from(number(node, "offset")?)?;
                let mut step_indices = BTreeSet::new();
                let steps = node["steps"]
                    .as_array()
                    .context("Source subclass steps")?
                    .iter()
                    .map(|step| {
                        let index = number(step, "index")?;
                        ensure!(step_indices.insert(index), "Duplicate source subclass step");
                        Ok(SubclassStep {
                            step: index,
                            name: string(step, "name")?,
                            description: string(step, "description")?,
                            foundation: foundations.get(&number(node, "index")?).copied(),
                            source: step.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                ensure!(!steps.is_empty(), "Source subclass node has no choices");
                nodes.push(SubclassNode {
                    node: index,
                    group_key: shared.u32(offset + 56)?,
                    node_flags: shared.u32(offset + 60)?,
                    row: i16::try_from(
                        node["row"]
                            .as_i64()
                            .context("Source subclass display row")?,
                    )?,
                    column: i16::try_from(
                        node["column"]
                            .as_i64()
                            .context("Source subclass display column")?,
                    )?,
                    record: hex::encode(shared.bytes::<64>(offset)?),
                    steps,
                });
            }
            let text_tag = strings.u32(text_row + 8)?;
            ensure!(
                self.reference(text_tag)? == 0x80803484,
                "Source subclass strings type differs"
            );
            let text = self.read(text_tag)?;
            let name = labels.optional_text(self, &text, 0x78)?.unwrap_or_default();
            let description = labels.optional_text(self, &text, 0x88)?.unwrap_or_default();
            inputs.insert(text_tag, text);
            let payloads = inputs.iter().map(|(&tag, payload)| {
                json!({"tag":tag,"bytes":payload.0.len(),"sha256":hex::encode(Sha256::digest(&payload.0))})
            }).collect::<Vec<_>>();
            let connections = super::super::connections::Graph::read(
                ability
                    .inputs
                    .get(&primary.graph)
                    .context("Source ability graph payload")?,
            )?;
            connections.validate_owners(&ability.inputs)?;
            let controller_occurrence = connections
                .components
                .iter()
                .position(|tag| *tag == controller)
                .context("Source subclass controller occurrence")?;
            let sequence_inputs = if primary.controller_class == 0x808025B8
                && primary.controller_definition == 0x80802A4A
                && primary.bank_class == 0x80802343
                && primary.bank_definition == 0x80802B06
            {
                Some(super::controller::Sequences::read(
                    self,
                    ability
                        .inputs
                        .get(&controller)
                        .context("Source Radiance controller payload")?,
                    &connections,
                    controller_occurrence,
                )?)
            } else {
                None
            };
            let lifecycle = connections
                .components
                .iter()
                .enumerate()
                .map(|(occurrence, tag)| {
                    let owner = ability
                        .inputs
                        .get(tag)
                        .context("Source lifecycle owner is absent")?;
                    Ok(json!({"occurrence":occurrence,"owner":tag,
                    "lifecycle":super::lifecycle::Lifecycle::read(owner)?}))
                })
                .collect::<Result<Vec<_>>>()?;
            let actions = sequence_inputs
                .as_ref()
                .map(|inputs| inputs.actions())
                .unwrap_or_default();
            let character_actions = actions
                .iter()
                .filter(|(_, owner_mode, _)| !owner_mode)
                .map(|(name, _, _)| *name)
                .collect::<BTreeSet<_>>();
            // This is the same shared playable-human lookup used by the character importer.
            // Alternate-owner actions belong to the selected weapon's first-person route.
            let animation_bank = if character_actions.is_empty() {
                None
            } else {
                Some(super::animation::read(
                    self,
                    0x80A7E159,
                    &character_actions,
                )?)
            };
            result.push(SubclassCatalog {
                item,
                name,
                description,
                grid: u16::try_from(number(&talents, "ordinal")?)?,
                ability_row: *ability_row,
                pattern: primary.pattern,
                graph: primary.graph,
                controller,
                controller_class: primary.controller_class,
                bank,
                abilities: selected_rows.iter().map(|row| abilities[row].catalog.clone()).collect(),
                nodes,
                source_inputs: json!({
                    "source":"d1_roi","builds":self.report.builds,"stamp":self.report.stamp,
                    "shared_grid":shared_tag,"client_grid":talents["client_grid"],"payloads":payloads,
                    "connections":connections,
                    "sequence_inputs":sequence_inputs,
                    "lifecycle":lifecycle,
                    "animation_actions":actions,
                    "character_animation_bank":animation_bank,
                    "foundation_scope":"fully_unlocked_prerequisite_closure",
                    "limits":["Every source subclass identity, node and referenced ability is retained. This is source intake, not native materialization.",
                        "Foundation roles describe qualified automatic and singleton prerequisite contributions in a fully unlocked build. Unqualified structures remain unset.",
                        "Declared ability graph owners and finished perk inputs are retained. Transitive script, effect and resource closure requires separate translation.",
                        "Native graph, stat and perk translations must cover every selected contribution before materialization."]
                }),
            });
        }
        result.sort_by(|a, b| (&a.name, a.item).cmp(&(&b.name, b.item)));
        self.unchanged()?;
        Ok(result)
    }
}

fn foundations(shared: &Payload, nodes: &[Value]) -> Result<BTreeMap<u32, bool>> {
    // Source build 56578 uses node +38 for exclusive groups. The upgrade gate at
    // 10437F0 reads step +30 as a prerequisite group and +34 as required progress.
    // The original 1042BA0 auto-selector admits flag-1 nodes at progress zero.
    // Singleton prerequisite groups are required in a full unlocked build even
    // though the original initializer does not select these visible nodes.
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut required = BTreeSet::new();
    let mut entries = Vec::new();
    for node in nodes {
        let index = number(node, "index")?;
        let at = usize::try_from(number(node, "offset")?)?;
        let group = shared.u32(at + 56)?;
        let flags = shared.u32(at + 60)?;
        groups.entry(group).or_default().push(flags);
        let steps = node["steps"].as_array().context("Source subclass steps")?;
        let mut requirement = None;
        for step in steps {
            let offset = usize::try_from(number(step, "offset")?)?;
            let key = shared.u32(offset + 0x30)?;
            if key != 0x811C9DC5 {
                required.insert(key);
            }
            requirement = Some((key, shared.u8(offset + 0x34)?));
        }
        if steps.len() == 1 && shared.u64(at + 8)? == 0 && shared.u64(at + 40)? == 0 {
            entries.push((
                index,
                group,
                flags,
                requirement.context("Source subclass requirement")?,
            ));
        }
    }
    ensure!(
        required.iter().all(|group| groups.contains_key(group)),
        "Source subclass prerequisite group has no node"
    );
    let mut result = BTreeMap::new();
    for (index, group, flags, (prerequisite, progress)) in entries {
        let members = &groups[&group];
        let foundation = match flags {
            1 if group == 0x811C9DC5 && prerequisite == 0x811C9DC5 && progress == 0 => true,
            0 if group != 0x811C9DC5 && members.iter().all(|flag| *flag == 0) => {
                if members.len() == 1 {
                    if !required.contains(&group) {
                        continue;
                    }
                    true
                } else {
                    false
                }
            }
            _ => continue,
        };
        result.insert(index, foundation);
    }
    Ok(result)
}

fn number(value: &Value, key: &str) -> Result<u32> {
    Ok(u32::try_from(
        value[key]
            .as_u64()
            .with_context(|| format!("Source subclass {key}"))?,
    )?)
}

fn string(value: &Value, key: &str) -> Result<String> {
    Ok(value[key]
        .as_str()
        .with_context(|| format!("Source subclass {key}"))?
        .to_owned())
}
