//! Resolve shader item identities independently of private socket-perk definitions.
use super::*;

impl RecipeLibrary {
    /// Read the current library, including shaders not explicitly enabled for a build.
    /// Matching uses the authored item identity, never the base shader or display name.
    pub(crate) fn shader_entries(
        &self,
        hashes: &BTreeSet<u32>,
    ) -> Result<Vec<RecipeLibraryEntry>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        self.scan()?.shader_entries(hashes)
    }
}

impl RecipeLibraryScan {
    pub(crate) fn shader_entries(
        &self,
        hashes: &BTreeSet<u32>,
    ) -> Result<Vec<RecipeLibraryEntry>, String> {
        let mut seen = BTreeSet::new();
        self.entries
            .iter()
            .filter(|entry| entry.kind == crate::ItemKind::Shader && hashes.contains(&entry.identity_hash))
            .map(|entry| {
                if !seen.insert(entry.identity_hash) {
                    return Err(format!(
                        "More than one shader recipe defines 0x{:08X}. Keep one recipe for this shader identity.",
                        entry.identity_hash
                    ));
                }
                Ok(entry.clone())
            })
            .collect()
    }
}
