//! Unbuilt native asset graphs use the ordinary decoder and renderer through a private reader.
use super::*;
use std::{collections::BTreeMap, sync::Arc};

/// Roots and effective dye parents after an authoring client links its local payloads.
pub struct LocalScene {
    pub entities: Vec<u32>,
    /// Local armor, cloth and suit slots 0 to 2, mapped to native dye parent tags.
    pub dyes: BTreeMap<usize, u32>,
    /// The native gear defaults the game binds when the item has no private dyes, as
    /// (channel key, dye material index). Used only while `dyes` is empty.
    pub stock_dyes: Vec<(i8, u16)>,
}

/// The dyes the gear path binds for an item that carries no private dyes: its definition's
/// own translation rows, composed the way the game composes them, before any plug.
pub fn stock_default_dyes(item: &[u8]) -> Vec<(i8, u16)> {
    crate::dyes::compose(
        &crate::catalog::translation_dye_rows(item),
        std::iter::empty(),
    )
}

type Reader = dyn Fn(&mut PackageManager) -> Result<LocalScene, String> + Send + Sync;

/// A content identity and deferred source reader. Construction does no filesystem work.
#[derive(Clone)]
pub struct LocalAppearance {
    key: String,
    read: Arc<Reader>,
}

impl std::fmt::Debug for LocalAppearance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalAppearance").field(&self.key).finish()
    }
}
impl PartialEq for LocalAppearance {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for LocalAppearance {}

impl LocalAppearance {
    /// `key` must change with the selected content and every choice the reader applies.
    /// The reader runs off the UI thread and may add private local packages to its manager.
    pub fn new(
        key: String,
        read: impl Fn(&mut PackageManager) -> Result<LocalScene, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            key,
            read: Arc::new(read),
        }
    }

    fn prepare(&self, packages: &Path) -> Result<(PackageManager, LocalScene), String> {
        let mut manager = PackageManager::new(
            packages,
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            None,
        )?;
        let scene = (self.read)(&mut manager)?;
        Ok((manager, scene))
    }

    pub(crate) fn clip_bytes(&self, packages: &Path, tag: u32) -> Result<Vec<u8>, String> {
        let (manager, _) = self.prepare(packages)?;
        assets::clip_bytes_with_manager(&manager, tag)
    }

    pub(crate) fn load(
        &self,
        packages: &Path,
        cancel: &Load,
        object: Option<u32>,
        clip: Option<u32>,
    ) -> Result<Model, String> {
        cancel.say("Reading imported appearance", 0, 0);
        let (manager, scene) = self.prepare(packages)?;
        if cancel.stopped() {
            return Err(CANCELLED.into());
        }
        if let Some(object) = object {
            return load_with_manager(&manager, object, cancel, clip);
        }
        let entities: BTreeSet<_> = scene.entities.into_iter().collect();
        if entities.is_empty() || entities.len() > 64 {
            return Err(
                "Imported appearance has no model roots or exceeds the object budget".into(),
            );
        }
        let mut result = Model::default();
        for (index, entity) in entities.into_iter().enumerate() {
            if cancel.stopped() {
                return Err(CANCELLED.into());
            }
            let mut model = load_with_manager(&manager, entity, cancel, None)?;
            if let Some(clip) = clip.filter(|tag| model.clips.iter().any(|c| c.tag == *tag)) {
                model = load_with_manager(&manager, entity, cancel, Some(clip))?;
            }
            appearance::append(&mut result, model, &format!("Part {}", index + 1))?;
        }
        if result.triangles.is_empty() {
            return Err("Imported appearance has no supported geometry".into());
        }
        let stock_dyes = scene.stock_dyes;
        let dyes = scene
            .dyes
            .into_iter()
            .map(|(slot, parent)| {
                crate::dyes::source::from_parent(&manager, parent).map(|source| (slot, source))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        crate::dyes::source::apply_with_manager(&manager, &dyes, &mut result, cancel)?;
        if dyes.is_empty() && !stock_dyes.is_empty() {
            // Converted programs multiply by the bound dye vectors, so the gear defaults the
            // game binds must be bound here too, or the surfaces draw black.
            let globals = manager.read_tag(crate::package_authoring::resolve_live_named_tag(
                &manager,
                "investment_globals",
                None,
            )?)?;
            let stock = appearance::Appearance {
                arrangement: 0,
                dyes: stock_dyes,
                dye_textures: Vec::new(),
            };
            appearance::apply_colors(&manager, &globals, &stock, &mut result)?;
            result.iridescence = texture::iridescence(&manager);
        }
        result.notices.sort();
        result.notices.dedup();
        Ok(result)
    }
}
