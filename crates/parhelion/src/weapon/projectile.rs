//! Values of the projectile a weapon fires.
//!
//! A weapon names the graph it fires in each variant block of its content owner (`+0xF0`). The
//! build copies that graph privately with these values, along with each graph below it that one
//! of them changes, and every block that fires the stock graph fires the copy. The stock graph and
//! every other weapon firing it stay as they are.
use sundial::package_authoring::runtime::WeaponRuntimeValueOverride;

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
