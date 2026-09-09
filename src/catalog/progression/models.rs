use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProgressionScope {
    Account,
    Character,
    Unreplicated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProgressionStepDefinition {
    #[serde(alias = "progress_total")]
    pub cost: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlock_flag: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_container: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProgressionRewardDefinition {
    pub rewarded_at_progression_level: i32,
    pub item_hash: u64,
    pub quantity: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_flag: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProgressionFactionDefinition {
    pub hash: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProgressionDefinition {
    pub definition_index: u16,
    pub hash: u64,
    pub scope: ProgressionScope,
    pub scope_slot: Option<u16>,
    pub repeat_last_step: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_value: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_units_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_container: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factions: Vec<ProgressionFactionDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ProgressionStepDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reward_items: Vec<ProgressionRewardDefinition>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObjectiveDef {
    pub hash: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub progress_description: String,
    /// Preferred display text retained for compatibility with older catalog caches.
    pub description: String,
    pub completion_value: i32,
    pub allow_overcompletion: bool,
    pub allow_negative_value: bool,
    pub allow_value_change_when_completed: bool,
    pub is_counting_downward: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_programs: Vec<Vec<[u32; 2]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_objective_indices: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intrinsic_perk_flag_definition_indices: Vec<u16>,
    pub owners: Vec<ObjectiveOwnerDef>,
    #[serde(default)]
    pub related_unlock_value_definition_index: Option<u16>,
}

impl ObjectiveDef {
    pub(crate) const fn maximum_value(&self) -> Option<i32> {
        if self.allow_overcompletion || self.is_counting_downward {
            None
        } else {
            Some(self.completion_value)
        }
    }

    pub(crate) const fn minimum_value(&self) -> Option<i32> {
        if self.allow_overcompletion || !self.is_counting_downward {
            None
        } else {
            Some(self.completion_value)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ObjectiveOwnerKind {
    InventoryItem,
    Milestone,
    Metric,
    Record,
    PresentationNode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObjectiveOwnerTraitDef {
    pub hash: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObjectiveOwnerDef {
    pub hash: u64,
    pub kind: ObjectiveOwnerKind,
    pub name: String,
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<ObjectiveOwnerTraitDef>,
    pub paths: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UnlockDefinition {
    pub hash: u64,
    pub code: u16,
    pub compact_slot: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tested_by: Vec<ProgressionContextDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_writers: Vec<UnlockWriter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum UnlockWriter {
    ProgressionStep {
        definition_index: u16,
        step_index: u16,
    },
    ProgressionLevel {
        definition_index: u16,
    },
    ValueCounter {
        programs: Vec<Vec<[u32; 2]>>,
    },
    /// The writer requires activity, item, or other client context not in this snapshot.
    Context {
        source: String,
    },
}

impl UnlockDefinition {
    pub(crate) const fn bank(&self) -> u8 {
        self.code.to_le_bytes()[0]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProgressionContextKind {
    InventoryItem,
    Collectible,
    Record,
    Objective,
    PresentationNode,
    Activity,
    ActivityAvailability,
    Location,
    LocationRelease,
    ExpressionMapping,
    Progression,
    Achievement,
    Requirement,
    ValueCounter,
    PackageExpression,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProgressionContextDef {
    pub hash: u64,
    pub kind: ProgressionContextKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_programs: Vec<Vec<[u32; 2]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_references: Vec<String>,
}

#[derive(Clone, Debug)]
pub(in crate::catalog) struct PresentationNodeDef {
    pub(super) hash: u64,
    pub(super) name: String,
    pub(super) parents: Vec<usize>,
    pub(super) objective_index: Option<usize>,
    pub(super) condition_references: ConditionReferences,
}

#[derive(Clone, Debug)]
pub(in crate::catalog) struct PendingProgressionContext {
    pub(super) hash: u64,
    pub(super) kind: ProgressionContextKind,
    pub(super) references: ConditionReferences,
    pub(super) paths: Vec<Vec<String>>,
}

pub(in crate::catalog) struct ProgressionPackageData<'a> {
    pub(super) manager: &'a PackageManager,
    pub(super) root: &'a [u8],
    pub(super) globals: &'a [u8],
    pub(super) localized_tags: &'a [TagHash],
    pub(super) localized_cache: &'a mut HashMap<u32, HashMap<u32, String>>,
}

impl<'a> ProgressionPackageData<'a> {
    pub(in crate::catalog) fn new(
        manager: &'a PackageManager,
        root: &'a [u8],
        globals: &'a [u8],
        localized_tags: &'a [TagHash],
        localized_cache: &'a mut HashMap<u32, HashMap<u32, String>>,
    ) -> Self {
        Self {
            manager,
            root,
            globals,
            localized_tags,
            localized_cache,
        }
    }
}
