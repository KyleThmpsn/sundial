//! Values of the projectile a weapon fires.
//!
//! A weapon names the graph it fires in each variant block of its content owner (`+0xF0`). The
//! build copies that graph privately with these values, along with each graph below it that one
//! of them changes, and every block that fires the stock graph fires the copy. The stock graph and
//! every other weapon firing it stay as they are.
use sundial::package_authoring::runtime::WeaponRuntimeValueOverride;

/// A repository-owned native projectile graph carried with an attached private perk.
pub use sundial::package_authoring::sandbox_perk::program::projectile::Dependency as Preset;

pub(crate) fn manifest(preset: Preset) -> &'static str {
    match preset {
        Preset::WolfpackRounds => include_str!("projectile/wolfpack_rounds.json"),
    }
}

/// The bundled preset that the projectile an imported graph carries under `root` is a byte for
/// byte copy of, every node with the preset's payload. An earlier perk import put such copies
/// into weapon graphs, and a selection of one fires the same graph as the preset.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn bundled_copy(graph: &std::path::Path, root: &str) -> Option<Preset> {
    use sha2::{Digest, Sha256};
    let manifest_bytes = std::fs::read(graph.join("asset-graph.json")).ok()?;
    let imported: serde_json::Value = serde_json::from_slice(&manifest_bytes).ok()?;
    let projectile = &imported["projectile"];
    if projectile["root"] != root {
        return None;
    }
    let nodes = projectile["nodes"].as_array()?;
    [Preset::WolfpackRounds].into_iter().find(|preset| {
        let Ok(bundled) = serde_json::from_str::<serde_json::Value>(manifest(*preset)) else {
            return false;
        };
        let Some(expected) = bundled["nodes"].as_array() else {
            return false;
        };
        bundled["root"] == root
            && expected.len() == nodes.len()
            && expected.iter().all(|node| {
                nodes
                    .iter()
                    .find(|copy| copy["symbol"] == node["symbol"])
                    .and_then(|copy| copy["file"].as_str())
                    // A verbatim directory path reads `/` as part of a name.
                    .map(|file| {
                        file.split('/')
                            .fold(graph.to_path_buf(), |path, part| path.join(part))
                    })
                    .and_then(|path| std::fs::read(path).ok())
                    .is_some_and(|bytes| {
                        node["payload_sha256"].as_str()
                            == Some(hex::encode(Sha256::digest(&bytes)).as_str())
                    })
            })
    })
}

/// Values set on the projectile a weapon fires, and the graph they were set for.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edits {
    /// The firing graph the values were set for. The build refuses them once the weapon fires
    /// another.
    pub graph: u32,
    /// Each value names its graph: the firing graph, or one it spawns.
    pub values: Vec<WeaponRuntimeValueOverride>,
}
