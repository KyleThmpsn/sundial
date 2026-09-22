//! Content provenance for browsing and structural loading checks for compilation.
//! Names and paths are hints. The native loading index is the build-time authority.

use crate::package_runtime::reader::PackageManager;

use super::catalog::Entry;

mod requirements;
pub use requirements::{Report, Requirement, inspect};

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

/// A cloned graph is enrolled by the compiler. Its existing component prerequisites must
/// already be covered by the native loading index. A missing UI cache never skips this check.
pub fn check(manager: &PackageManager, graph: u32, role: &str) -> Result<(), String> {
    inspect(manager, graph)?.check(role, true)
}

/// Direct references also require the graph itself to be loaded.
pub fn check_reference(manager: &PackageManager, graph: u32, role: &str) -> Result<(), String> {
    inspect(manager, graph)?.check(role, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox_perk::projectile::{Kind, catalog::Context};

    fn entry(paths: &[&str]) -> Entry {
        Entry {
            source_hint: None,
            graph: 7,
            kind: Kind::Projectile,
            object_type: 18,
            owners: Vec::new(),
            package: "sandbox".into(),
            native_name: None,
            native_paths: Vec::new(),
            contexts: paths
                .iter()
                .map(|path| Context {
                    graph: 30,
                    owner: 30,
                    offset: 0,
                    path: (*path).to_owned(),
                    name_evidence: None,
                    item: None,
                    perk: None,
                    depth: 2,
                })
                .collect(),
            perk_indices: Vec::new(),
        }
    }

    #[test]
    fn paths_say_where_content_loads() {
        assert_eq!(
            classify_path(
                "content\\sandbox\\weapons\\player\\grenade_launchers\\projectiles\\grenade_launcher_wave.pattern.tft"
            ),
            Some(Residency::Player)
        );
        assert_eq!(
            classify_path(
                "content\\sandbox\\abilities\\player\\supers\\nova_bomb\\nova_bomb_ability.pattern.tft"
            ),
            Some(Residency::Player)
        );
        assert_eq!(
            classify_path(
                "content\\sandbox\\v420\\weapons\\_prime\\attach_variant_perk\\outbreak_attach_hopon.pattern.tft"
            ),
            Some(Residency::Player)
        );
        assert_eq!(
            classify_path(
                "content\\activities\\v400\\sandbox_custom\\characters\\taken\\_factions\\base\\taken_centurion\\taken_centurion_v400.pattern.tft"
            ),
            Some(Residency::Activity)
        );
        assert_eq!(
            classify_path(
                "content\\sandbox\\characters\\taken\\_factions\\base\\taken_psion\\taken_psion.pattern.tft"
            ),
            Some(Residency::Activity)
        );
        assert_eq!(
            classify_path(
                "content\\sandbox\\abilities\\cabal\\scimitar\\sequences\\almighty_scimitar_activation.sequence.tft"
            ),
            Some(Residency::Activity)
        );
        assert_eq!(
            classify_path(
                "content/environments/specops/ketch_arrival/fx/dust_shockwave_rush.fx_sequence.tft"
            ),
            Some(Residency::Activity)
        );
        assert_eq!(
            classify_path("content/common/native/sandbox/label_globals.label_globals.tft"),
            None
        );
        assert_eq!(classify_path(""), None);
    }

    #[test]
    fn entries_follow_their_strongest_evidence() {
        // The Taken Centurion projectile: every ancestor is enemy or activity content.
        let taken = entry(&[
            "content\\activities\\v400\\sandbox_custom\\characters\\taken\\_factions\\base\\taken_centurion\\taken_centurion_v400.pattern.tft",
            "content\\sandbox\\characters\\taken\\_factions\\base\\taken_phalanx\\taken_phalanx.pattern.tft",
        ]);
        assert_eq!(classify(&taken), Residency::Activity);
        assert!(!classify(&taken).allowed());
        // Nova Bomb: reached from a player super pattern.
        let nova = entry(&[
            "content\\sandbox\\abilities\\player\\supers\\nova_bomb\\nova_bomb_ability.pattern.tft",
        ]);
        assert_eq!(classify(&nova), Residency::Player);
        // A weapon pattern or a stock perk settles it without a path.
        let mut fired = entry(&[]);
        fired.contexts.push(Context {
            item: Some(0x2B50_ED7D),
            ..fired.contexts.first().cloned().unwrap_or_else(|| Context {
                graph: 30,
                owner: 30,
                offset: 0,
                path: String::new(),
                name_evidence: None,
                item: None,
                perk: None,
                depth: 2,
            })
        });
        assert_eq!(classify(&fired), Residency::Player);
        let mut used = entry(&[]);
        used.perk_indices.push(1178);
        assert_eq!(classify(&used), Residency::Player);
        // Player evidence outranks activity evidence, and no evidence stays open.
        let shared = entry(&[
            "content\\sandbox\\characters\\vex\\base\\sequences\\soft_death.sequence.tft",
            "content\\sandbox\\weapons\\player\\shared\\muzzle.pattern.tft",
        ]);
        assert_eq!(classify(&shared), Residency::Player);
        assert_eq!(classify(&entry(&[])), Residency::Unverified);
        assert!(classify(&entry(&[])).allowed());
    }
}
