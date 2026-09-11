//! Connected binding groups, including bindings that span more than one owner.
use super::*;

/// Every binding that must be selected together to replace a complete connected owner group.
/// A multi-resource binding can join otherwise separate owners, so a one-hop lookup is insufficient.
pub fn coupled_weapon_component_bindings(entity: &[u8], selected: u32) -> Result<Vec<u32>, String> {
    let aliases = weapon_component_aliases(entity)?;
    if !aliases.iter().any(|alias| alias.binding_hash == selected) {
        return Err(format!(
            "The runtime baseline has no binding 0x{selected:08X}"
        ));
    }
    let mut bindings = BTreeSet::from([selected]);
    let mut owners = BTreeSet::new();
    loop {
        let previous = (bindings.len(), owners.len());
        for alias in &aliases {
            if bindings.contains(&alias.binding_hash) {
                owners.insert(alias.owner_tag);
            }
        }
        for alias in &aliases {
            if owners.contains(&alias.owner_tag) {
                bindings.insert(alias.binding_hash);
            }
        }
        if previous == (bindings.len(), owners.len()) {
            return Ok(bindings.into_iter().collect());
        }
    }
}
