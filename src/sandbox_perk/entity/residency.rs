//! Content provenance for browsing and structural loading checks for compilation.
//! Names and paths are hints. The native loading index is the build-time authority.

use super::catalog::Entry;

mod requirements;
pub use requirements::{
    Report, Requirement, additions, inspect, owner_requirements, resource_additions,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Residency {
    /// Player weapon or ability content, loaded in every activity.
    Player,
    /// Nothing names the asset, so nothing says where it loads.
    Unverified,
    /// Enemy, activity, environment or cinematic content that loads with its activity.
    Activity,
}

impl Residency {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Player => "Player Content",
            Self::Unverified => "Unverified",
            Self::Activity => "Activity Content",
        }
    }

    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Player => {
                "Used by player weapons or abilities. The build checks its loading requirements separately."
            }
            Self::Unverified => {
                "Nothing names this asset, so where it loads is unknown. Test it in game before sharing the perk."
            }
            Self::Activity => {
                "Used by enemies or activities. It may need dependencies that are not loaded with the player. The build checks its loading requirements separately."
            }
        }
    }

    /// Whether a weapon perk may reference the asset.
    #[must_use]
    pub const fn allowed(self) -> bool {
        !matches!(self, Self::Activity)
    }
}

/// Content roots whose assets load with an activity rather than with the player.
const ACTIVITY_ROOTS: [&str; 9] = [
    "activities",
    "activities_d2",
    "environments",
    "raids",
    "pvecomp",
    "ui",
    "audio",
    "fx",
    "cinematics",
];

/// Sandbox folders that hold enemy and vehicle content.
const ACTIVITY_SANDBOX: [&str; 2] = ["characters", "vehicles"];

/// Where an asset loads, judged from its own path, the paths of the resources that name it,
/// and the weapons and perks that reach it.
#[must_use]
pub fn classify(entry: &Entry) -> Residency {
    if !entry.perk_indices.is_empty()
        || entry
            .contexts
            .iter()
            .any(|context| context.item.is_some() || context.perk.is_some())
    {
        return Residency::Player;
    }
    let verdicts = entry
        .native_paths
        .iter()
        .chain(entry.contexts.iter().map(|context| &context.path))
        .filter_map(|path| classify_path(path))
        .collect::<Vec<_>>();
    if verdicts.contains(&Residency::Player) {
        Residency::Player
    } else if verdicts.contains(&Residency::Activity) {
        Residency::Activity
    } else {
        Residency::Unverified
    }
}

/// The verdict one content path carries, or `None` for shared folders that say nothing.
#[must_use]
pub fn classify_path(path: &str) -> Option<Residency> {
    let segments = path
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let mut rest = segments.as_slice();
    if rest.first().is_some_and(|first| first == "content") {
        rest = &rest[1..];
    }
    let root = rest.first()?;
    let has = |name: &str| rest.iter().any(|segment| segment == name);
    if has("player") {
        return Some(Residency::Player);
    }
    if root == "sandbox" {
        if has("weapons") {
            return Some(Residency::Player);
        }
        if ACTIVITY_SANDBOX.iter().any(|folder| has(folder)) || has("abilities") {
            return Some(Residency::Activity);
        }
        return None;
    }
    ACTIVITY_ROOTS
        .contains(&root.as_str())
        .then_some(Residency::Activity)
}
