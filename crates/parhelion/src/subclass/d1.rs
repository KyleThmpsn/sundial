//! Complete source selections remain saved independently of native translation readiness.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    AttunementPath, EntryEdits, Place, SubclassAbilities, SubclassChoice, SubclassPathNode, layout,
};
use crate::{ItemKind, WeaponRecipe};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub node: u32,
    pub step: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    #[default]
    Unqualified,
    SharedAbilities,
    PathChoice,
    Foundation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub key: u32,
    pub role: GroupRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    pub path: AttunementPath,
    pub name: String,
    pub choices: Vec<Choice>,
}

/// D1 has no class-ability column. A native subclass keeps that explicit D2 addition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassAbilityPolicy {
    RetainNative,
}

/// The native class-base entry is outside the editable talent grid. A complete import
/// must qualify its role before retaining it as an explicit target-platform adaptation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseMeleePolicy {
    #[default]
    Unqualified,
    RetainNative,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub builds: Vec<String>,
    pub stamp: String,
    /// The complete catalog object, including raw node and step records and input fingerprints.
    /// Unknown source fields are preserved rather than interpreted as native authoring fields.
    pub source: Value,
    pub source_sha256: String,
    pub groups: Vec<Group>,
    pub paths: Vec<Path>,
    pub class_abilities: ClassAbilityPolicy,
    pub base_melee: BaseMeleePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    applied: Option<Applied>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Applied {
    plan_sha256: String,
    abilities_sha256: String,
}

/// A selected step must account for its selection semantics and every source contribution.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Record {
    Selection,
    Grant { variant: usize, index: usize },
    Perk { variant: usize, index: usize },
    Stat { variant: usize, index: usize },
    Appearance,
}

/// One native entry contribution. Several source selections may contribute to the same
/// path node, but their donor and scalar fields must agree. Lists merge without duplicates.
pub struct Placement {
    pub entry: u8,
    pub source: u32,
    pub source_entry: u8,
    pub edits: EntryEdits,
}

/// The native producer supplies complete, source-pinned output for one selected step.
/// This is an in-memory compilation result, not a user-set readiness flag in the saved plan.
pub struct Translation {
    pub choice: Choice,
    pub path: Option<AttunementPath>,
    pub source_sha256: String,
    pub consumed: Vec<Record>,
    pub placements: Vec<Placement>,
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}

impl Plan {
    #[cfg(feature = "d2-model-importer")]
    pub fn from_source(
        source: &parhelion_import::d1_roi::ability::SubclassCatalog,
        builds: &[String],
        stamp: &str,
    ) -> Result<Self, String> {
        let snapshot = serde_json::to_value(source).map_err(|error| error.to_string())?;
        let groups = source
            .nodes
            .iter()
            .map(|node| node.group_key)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| Group {
                key,
                role: GroupRole::Unqualified,
            })
            .collect();
        let paths = [
            (AttunementPath::Top, "Radiant Skin"),
            (AttunementPath::Bottom, "Song of Flame"),
            (AttunementPath::Middle, "Fireborn"),
        ]
        .into_iter()
        .map(|(path, name)| Path {
            path,
            name: name.into(),
            choices: Vec::new(),
        })
        .collect();
        let plan = Self {
            version: 1,
            builds: builds.to_vec(),
            stamp: stamp.into(),
            source_sha256: digest(&snapshot)?,
            source: snapshot,
            groups,
            paths,
            class_abilities: ClassAbilityPolicy::RetainNative,
            base_melee: BaseMeleePolicy::Unqualified,
            applied: None,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn nodes(&self) -> Result<&Vec<Value>, String> {
        self.source["nodes"]
            .as_array()
            .ok_or_else(|| "D1 subclass source has no node table".into())
    }

    fn choices(&self, key: u32) -> Result<Vec<Choice>, String> {
        let mut result = Vec::new();
        for node in self
            .nodes()?
            .iter()
            .filter(|node| node["group_key"].as_u64() == Some(u64::from(key)))
        {
            let index = number(node, "node")?;
            for step in node["steps"]
                .as_array()
                .ok_or("D1 source node has no steps")?
            {
                result.push(Choice {
                    node: index,
                    step: number(step, "step")?,
                });
            }
        }
        Ok(result)
    }

    fn step(&self, choice: Choice) -> Result<&Value, String> {
        self.nodes()?
            .iter()
            .find(|node| node["node"].as_u64() == Some(u64::from(choice.node)))
            .and_then(|node| node["steps"].as_array())
            .and_then(|steps| {
                steps
                    .iter()
                    .find(|step| step["step"].as_u64() == Some(u64::from(choice.step)))
            })
            .ok_or_else(|| format!("D1 source choice {}:{} is absent", choice.node, choice.step))
    }

    pub fn choice_fingerprint(&self, choice: Choice) -> Result<String, String> {
        digest(self.step(choice)?)
    }

    pub fn records(&self, choice: Choice) -> Result<Vec<Record>, String> {
        let step = &self.step(choice)?["source"];
        let mut records = vec![Record::Selection];
        let variants = step["variants"]
            .as_array()
            .ok_or("D1 source choice has no variant table")?;
        for (variant, value) in variants.iter().enumerate() {
            for (name, kind) in [("ability_grants", 0), ("perks", 1), ("stat_adjustments", 2)] {
                for index in 0..value[name]
                    .as_array()
                    .ok_or_else(|| format!("D1 source choice has no {name} table"))?
                    .len()
                {
                    records.push(match kind {
                        0 => Record::Grant { variant, index },
                        1 => Record::Perk { variant, index },
                        _ => Record::Stat { variant, index },
                    });
                }
            }
        }
        if step["appearance_choices"]
            .as_u64()
            .ok_or("D1 source appearance choices are absent")?
            != 0
        {
            records.push(Record::Appearance);
        }
        Ok(records)
    }

    /// Structural validity permits saving an incomplete source plan for further preparation.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.builds.is_empty()
            || self.stamp.is_empty()
            || self.source_sha256 != digest(&self.source)?
        {
            return Err("D1 subclass source identity or fingerprint changed".into());
        }
        let nodes = self.nodes()?;
        if nodes.is_empty() {
            return Err("D1 subclass source grid is empty".into());
        }
        let mut keys = BTreeSet::new();
        let mut indices = BTreeSet::new();
        for node in nodes {
            if !indices.insert(number(node, "node")?) {
                return Err("D1 source repeats a node".into());
            }
            keys.insert(number(node, "group_key")?);
            let steps = node["steps"]
                .as_array()
                .filter(|steps| !steps.is_empty())
                .ok_or("D1 source node has no steps")?;
            let mut seen = BTreeSet::new();
            for step in steps {
                if !seen.insert(number(step, "step")?) {
                    return Err("D1 source repeats a step".into());
                }
            }
        }
        if self.groups.len() != keys.len()
            || self
                .groups
                .iter()
                .map(|group| group.key)
                .collect::<BTreeSet<_>>()
                != keys
        {
            return Err("D1 subclass plan must retain every source group once".into());
        }
        if self.paths.len() != 3
            || self
                .paths
                .iter()
                .map(|path| path.path)
                .collect::<BTreeSet<_>>()
                != AttunementPath::ALL.into_iter().collect()
        {
            return Err("D1 subclass plan needs all three attunement paths".into());
        }
        for path in &self.paths {
            if !super::edits::text_is_valid(&path.name, super::edits::NAME_LIMIT) {
                return Err("D1 attunement name is empty or too long".into());
            }
            let mut seen = BTreeSet::new();
            for choice in &path.choices {
                self.step(*choice)?;
                if !seen.insert(*choice) {
                    return Err("D1 attunement repeats a source choice".into());
                }
            }
        }
        Ok(())
    }

    fn required(&self) -> Result<BTreeSet<(Option<AttunementPath>, Choice)>, String> {
        self.validate()?;
        if self.base_melee == BaseMeleePolicy::Unqualified {
            return Err("D1 subclass preparation needs a qualified base-melee adaptation".into());
        }
        let mut result = BTreeSet::new();
        for group in &self.groups {
            let choices = self.choices(group.key)?;
            match group.role {
                GroupRole::Unqualified => {
                    return Err(format!(
                        "D1 source group {:08X} still needs qualified selection and contribution translation",
                        group.key
                    ));
                }
                GroupRole::SharedAbilities | GroupRole::Foundation => {
                    for choice in choices {
                        if group.role == GroupRole::Foundation
                            && self.step(choice)?["foundation"].as_bool() != Some(true)
                        {
                            return Err(format!(
                                "D1 source choice {}:{} has no qualified foundation role",
                                choice.node, choice.step
                            ));
                        }
                        result.insert((None, choice));
                    }
                }
                GroupRole::PathChoice => {
                    for path in &self.paths {
                        let selected = path
                            .choices
                            .iter()
                            .filter(|choice| choices.contains(choice))
                            .copied()
                            .collect::<Vec<_>>();
                        if selected.len() != 1 {
                            return Err(format!(
                                "{} needs one choice from D1 source group {:08X}",
                                path.name, group.key
                            ));
                        }
                        result.insert((Some(path.path), selected[0]));
                    }
                }
            }
        }
        for path in &self.paths {
            if path
                .choices
                .iter()
                .any(|choice| !result.contains(&(Some(path.path), *choice)))
            {
                return Err(format!(
                    "{} has a choice outside its qualified path groups",
                    path.name
                ));
            }
        }
        Ok(result)
    }

    /// Atomically project complete source contributions into the native grid. A missing
    /// selected graph, perk, stat or appearance contribution leaves the whole draft unchanged.
    pub fn materialize(
        &self,
        recipe: &mut WeaponRecipe,
        translations: &[Translation],
    ) -> Result<(), String> {
        if recipe.kind != ItemKind::Subclass {
            return Err("D1 subclass plans require a subclass recipe".into());
        }
        let selected = self.required()?;
        let supplied = translations
            .iter()
            .map(|item| (item.path, item.choice))
            .collect::<BTreeSet<_>>();
        if supplied != selected || supplied.len() != translations.len() {
            return Err(
                "D1 subclass preparation is incomplete or repeats a selected contribution".into(),
            );
        }
        let base = recipe
            .donor
            .item_hash
            .parse_u32()
            .map_err(|error| error.to_string())?;
        let mut entries = BTreeMap::<u8, Placement>::new();
        for translated in translations {
            let expected = self
                .records(translated.choice)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            if translated.source_sha256 != self.choice_fingerprint(translated.choice)?
                || translated.consumed.len() != expected.len()
                || translated.consumed.iter().copied().collect::<BTreeSet<_>>() != expected
            {
                return Err(format!(
                    "D1 source choice {}:{} has incomplete or stale translation",
                    translated.choice.node, translated.choice.step
                ));
            }
            if expected.len() > 1
                && !translated
                    .placements
                    .iter()
                    .any(|placement| has_behavior(&placement.edits))
            {
                return Err("A selected D1 contribution has only presentation metadata".into());
            }
            for placement in &translated.placements {
                let place = place(placement.entry)?;
                if layout::CLASS_ABILITIES.contains(&placement.entry)
                    || translated
                        .path
                        .is_some_and(|path| !path.entries().contains(&placement.entry))
                {
                    return Err("D1 contribution is outside its selected path or replaces the retained class abilities".into());
                }
                placement
                    .edits
                    .validate(&place.label(), placement.source_entry)?;
                if let Some(current) = entries.get_mut(&placement.entry) {
                    if (current.source, current.source_entry)
                        != (placement.source, placement.source_entry)
                    {
                        return Err(
                            "Selected D1 contributions disagree on a native entry donor".into()
                        );
                    }
                    merge(&mut current.edits, &placement.edits)?;
                } else {
                    entries.insert(
                        placement.entry,
                        Placement {
                            entry: placement.entry,
                            source: placement.source,
                            source_entry: placement.source_entry,
                            edits: placement.edits.clone(),
                        },
                    );
                }
            }
        }
        for entry in Place::editable()
            .map(super::place_entry)
            .filter(|entry| !layout::CLASS_ABILITIES.contains(entry))
        {
            if !entries.contains_key(&entry) {
                return Err(format!(
                    "D1 preparation does not supply native entry {entry}"
                ));
            }
        }
        for entry in layout::MOVEMENT
            .into_iter()
            .chain(layout::GRENADES)
            .chain([layout::SUPER])
            .chain(layout::PATH_ABILITIES)
        {
            if entries
                .get(&entry)
                .is_none_or(|placement| placement.edits.imported_ability.is_none())
            {
                return Err(format!(
                    "D1 preparation has no complete source ability graph for entry {entry}"
                ));
            }
        }
        let old = recipe.overrides.subclass_abilities.as_ref();
        let mut abilities = SubclassAbilities::default();
        for entry in layout::CLASS_ABILITIES {
            abilities.set_ability(
                base,
                old.map_or_else(
                    || SubclassChoice::stock(entry, base, entry),
                    |abilities| abilities.ability(base, entry),
                ),
            );
        }
        for placement in entries.into_values() {
            match place(placement.entry)? {
                Place::Ability(entry) => abilities.set_ability(
                    base,
                    SubclassChoice {
                        entry,
                        source: placement.source,
                        source_entry: placement.source_entry,
                        edits: placement.edits,
                    },
                ),
                Place::Node(path, position) => {
                    let Place::Node(source_path, source_position) = place(placement.source_entry)?
                    else {
                        return Err("D1 path node has an incompatible native source entry".into());
                    };
                    abilities.set_path_node(
                        path,
                        base,
                        SubclassPathNode {
                            position,
                            source: placement.source,
                            source_path,
                            source_position,
                            edits: placement.edits,
                        },
                    );
                }
            }
        }
        for path in &self.paths {
            abilities.set_path_name(path.path, base, Some(path.name.clone()));
        }
        abilities.validate()?;
        let mut saved = self.clone();
        saved.applied = None;
        saved.applied = Some(Applied {
            plan_sha256: digest(&saved)?,
            abilities_sha256: digest(&abilities)?,
        });
        abilities.d1 = Some(saved);
        let mut prepared = recipe.clone();
        prepared.overrides.subclass_abilities = Some(abilities);
        prepared.validate().map_err(|error| error.to_string())?;
        *recipe = prepared;
        Ok(())
    }

    pub(super) fn ready(&self, abilities: &SubclassAbilities) -> Result<(), String> {
        let Some(applied) = &self.applied else {
            return Err("D1 subclass plan is saved but its selected behavior has not been completely prepared".into());
        };
        self.required()?;
        let mut plan = self.clone();
        plan.applied = None;
        let mut output = abilities.clone();
        output.d1 = None;
        if digest(&plan)? != applied.plan_sha256 || digest(&output)? != applied.abilities_sha256 {
            return Err(
                "D1 subclass selections or prepared entries changed and need preparation again"
                    .into(),
            );
        }
        Ok(())
    }
}

fn number(value: &Value, field: &str) -> Result<u32, String> {
    value[field]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("D1 source {field} is absent or out of range"))
}

fn place(entry: u8) -> Result<Place, String> {
    Place::editable()
        .find(|place| super::place_entry(*place) == entry)
        .ok_or_else(|| format!("D1 preparation names unsupported native entry {entry}"))
}

fn has_behavior(edits: &EntryEdits) -> bool {
    edits.imported_ability.is_some()
        || !edits.added_perks.is_empty()
        || !edits.custom_perks.is_empty()
        || !edits.modifiers.is_empty()
        || !edits.parameters.is_empty()
        || !edits.ability_values.is_empty()
        || !edits.bank_values.is_empty()
        || edits.extra_charges != 0
        || edits.recharge_bits != 0
}

fn merge(target: &mut EntryEdits, added: &EntryEdits) -> Result<(), String> {
    let mut merged = serde_json::to_value(&*target).map_err(|error| error.to_string())?;
    let extra = serde_json::to_value(added).map_err(|error| error.to_string())?;
    let fields = merged
        .as_object_mut()
        .ok_or("Prepared entry is not an object")?;
    for (key, value) in extra.as_object().ok_or("Prepared entry is not an object")? {
        match fields.get_mut(key) {
            None => {
                fields.insert(key.clone(), value.clone());
            }
            Some(current) if current == value => {}
            Some(Value::Array(current)) if value.is_array() => {
                for value in value.as_array().into_iter().flatten() {
                    if !current.contains(value) {
                        current.push(value.clone());
                    }
                }
            }
            Some(_) => return Err(format!("Selected D1 contributions disagree on {key}")),
        }
    }
    *target = serde_json::from_value(merged).map_err(|error| error.to_string())?;
    Ok(())
}
