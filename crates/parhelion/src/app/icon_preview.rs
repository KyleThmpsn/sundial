//! The authored icon's preview and editors: its layers, its rarity plate and the artwork
//! window.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AuthoredIconPreviewKey {
    pub(super) corner_icon: Option<crate::presentation::Artwork>,
    pub(super) item_hash: u32,
    pub(super) container_tag: u32,
    pub(super) rarity: crate::AuthoredWeaponRarity,
    pub(super) edit: crate::WeaponIconEdit,
    /// A subclass icon, shown without a rarity plate or watermark.
    pub(super) plain: bool,
    /// The art the icon shows in place of its image: a vehicle's silhouette or a generated icon.
    pub(super) art: Option<crate::icon_art::Request>,
}

impl AuthoredIconPreviewKey {
    /// Whether both previews read the same layers from the packages, whatever their edits. A
    /// generated icon's HUD color is drawn, not read, so the color can change without a read.
    pub(super) fn same_layers(&self, other: &Self) -> bool {
        self.container_tag == other.container_tag
            && self.rarity == other.rarity
            && self.corner_icon == other.corner_icon
            && self.plain == other.plain
            && match (&self.art, &other.art) {
                (Some(art), Some(other)) => art.same_reads(other),
                (art, other) => art.is_none() && other.is_none(),
            }
    }
}

pub(super) fn effective_icon_rarity(
    authored: Option<RecipeRarity>,
    inherited: Option<WeaponRarity>,
) -> Option<crate::AuthoredWeaponRarity> {
    use crate::AuthoredWeaponRarity as R;
    authored.map(Into::into).or_else(|| match inherited? {
        WeaponRarity::Common => Some(R::Common),
        WeaponRarity::Uncommon => Some(R::Uncommon),
        WeaponRarity::Rare => Some(R::Rare),
        WeaponRarity::Legendary => Some(R::Legendary),
        WeaponRarity::Exotic => Some(R::Exotic),
        WeaponRarity::Unknown => None,
    })
}

/// An icon's layers as read, with the art its image gives way to.
pub(super) type ReadIconLayers = Result<(IconLayers, Option<crate::icon_art::Read>), String>;

/// A rendered icon, or why it could not render, with what it was rendered from.
pub(super) enum AuthoredIconPreview<Key = AuthoredIconPreviewKey> {
    Ready {
        key: Key,
        texture: egui::TextureHandle,
    },
    Failed {
        key: Key,
        error: String,
    },
}

impl PackageAuthoringApp {
    pub(super) fn draw_icon_editor(&mut self, ctx: &egui::Context) {
        if self.build_receiver.is_some() || self.install_receiver.is_some() {
            return;
        }
        let action = self
            .icon_editor
            .as_mut()
            .and_then(|editor| editor.show(ctx));
        match action {
            Some(WeaponIconEditorAction::Apply(edit)) => {
                if self.recipe.overrides.icon_edit != edit {
                    self.recipe.overrides.icon_edit = edit;
                    self.recipe_dirty = true;
                    self.invalidate_results();
                }
                self.icon_editor = None;
            }
            Some(WeaponIconEditorAction::Cancel) => self.icon_editor = None,
            None => {}
        }
    }

    pub(super) fn draw_artwork_editor(&mut self, ctx: &egui::Context) {
        if !self.presentation_editor.editing()
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        let icon = (|| {
            let donor = self
                .recipe
                .icon_donor
                .as_ref()
                .or(self.recipe.presentation_donor.as_ref())
                .unwrap_or(&self.recipe.donor);
            let hash = donor.item_hash.parse_u32().ok()?;
            let tag = self.catalog.as_ref()?.weapon_icon_container(hash)?;
            // The watermark's preview shows the art the icon takes, which it never saves.
            Some((
                TagHash(tag),
                self.authored_icon_rarity()?,
                crate::icon_art::edited(
                    self.icon_art_request().as_ref(),
                    &self.recipe.overrides.icon_edit,
                    self.icon_art().as_ref(),
                ),
            ))
        })();
        if self.presentation_editor.show(
            ctx,
            &mut self.recipe.overrides,
            &self.packages,
            self.catalog.as_ref(),
            icon,
        ) {
            self.recipe_dirty = true;
            self.authored_icon_preview = None;
            self.invalidate_results();
        }
    }

    /// What the recipe's icon shows in place of its image, as its previews read and draw it.
    pub(super) fn icon_art_request(&self) -> Option<crate::icon_art::Request> {
        crate::icon_art::Art::of(&self.recipe).map(|art| art.request(&self.subclasses))
    }

    /// The art the icon shows in place of its image, as the icon card last read it and drawn in
    /// the recipe's current colors, for the icon editors to start from. `None` until the card has
    /// read it.
    pub(super) fn icon_art(&self) -> Option<crate::icon_edit::ImportedIcon> {
        let request = self.icon_art_request()?;
        let (read_for, Ok((_, Some(read)))) = self.authored_icon_layers.as_ref()? else {
            return None;
        };
        read_for
            .art
            .as_ref()
            .filter(|read_for| read_for.same_reads(&request))?;
        read.draw(request.look()).ok().flatten()
    }

    pub(super) fn authored_icon_preview(
        &mut self,
        context: &egui::Context,
        item_hash: u32,
        container_tag: TagHash,
    ) -> Result<Option<egui::TextureHandle>, String> {
        let rarity = self
            .authored_icon_rarity()
            .ok_or("Choose a base item to set the icon rarity")?;
        let edit = &self.recipe.overrides.icon_edit;
        let key = AuthoredIconPreviewKey {
            corner_icon: self.recipe.overrides.corner_icon.clone(),
            item_hash,
            container_tag: u32::from(container_tag),
            rarity,
            edit: edit.clone(),
            plain: self.recipe.kind == crate::ItemKind::Subclass,
            art: self.icon_art_request(),
        };
        let cached_matches = match self.authored_icon_preview.as_ref() {
            Some(AuthoredIconPreview::Ready { key: cached, .. })
            | Some(AuthoredIconPreview::Failed { key: cached, .. }) => *cached == key,
            None => false,
        };
        if !cached_matches {
            // An edit alone composes the layers already read, so a shader drawing its icon from
            // its dyes never opens the packages again as its values change. Until new layers
            // arrive, the card keeps the icon it shows for the same icon, so a new symbol or art
            // does not flash the stock icon. Another icon shows its stock icon until then.
            let Some(layers) = self.icon_layers(context, &key, container_tag) else {
                return Ok(match self.authored_icon_preview.as_ref() {
                    Some(AuthoredIconPreview::Ready {
                        key: shown,
                        texture,
                    }) if shown.item_hash == key.item_hash
                        && shown.container_tag == key.container_tag =>
                    {
                        Some(texture.clone())
                    }
                    _ => None,
                });
            };
            let image = match &layers.1 {
                Ok((layers, art)) => art
                    .as_ref()
                    .map(|art| {
                        art.draw(key.art.as_ref().map_or_else(
                            crate::icon_art::Look::default,
                            crate::icon_art::Request::look,
                        ))
                    })
                    .transpose()
                    .map(Option::flatten)
                    .and_then(|art| {
                        layers.render(&crate::icon_art::edited(
                            key.art.as_ref(),
                            &key.edit,
                            art.as_ref(),
                        ))
                    }),
                Err(error) => Err(error.clone()),
            };
            self.authored_icon_layers = Some(layers);
            self.authored_icon_preview = Some(match image {
                Ok(image) => AuthoredIconPreview::Ready {
                    key,
                    texture: context.load_texture(
                        format!("parhelion-authored-icon-{item_hash:08X}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    ),
                },
                Err(error) => AuthoredIconPreview::Failed { key, error },
            });
        }
        match self.authored_icon_preview.as_ref() {
            Some(AuthoredIconPreview::Ready { texture, .. }) => Ok(Some(texture.clone())),
            Some(AuthoredIconPreview::Failed { error, .. }) => Err(error.clone()),
            None => Ok(None),
        }
    }

    /// The layers `key` composes, once read. Reading them opens the packages, which takes
    /// seconds, so a worker reads them and this returns `None` until they arrive.
    pub(super) fn icon_layers(
        &mut self,
        context: &egui::Context,
        key: &AuthoredIconPreviewKey,
        container_tag: TagHash,
    ) -> Option<(AuthoredIconPreviewKey, ReadIconLayers)> {
        if let Some((loading, receiver)) = &self.authored_icon_loading {
            let finished = match receiver.try_recv() {
                Ok(layers) => Some(layers),
                Err(TryRecvError::Disconnected) => Some(Err("The icon loader stopped.".to_owned())),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(layers) = finished {
                self.authored_icon_layers = Some((loading.clone(), layers));
                self.authored_icon_loading = None;
            }
        }
        match self.authored_icon_layers.take() {
            Some((read, layers)) if read.same_layers(key) => return Some((read, layers)),
            other => self.authored_icon_layers = other,
        }
        let loading = self
            .authored_icon_loading
            .as_ref()
            .is_some_and(|(loading, _)| loading.same_layers(key));
        if !loading {
            // A load for other layers still finishes, and its result is dropped with its receiver.
            let (sender, receiver) = mpsc::channel();
            let packages = self.packages.clone();
            let corner = key.corner_icon.clone();
            let (rarity, plain, art) = (key.rarity, key.plain, key.art.clone());
            // An install waits for the read, which keeps package files open until it returns.
            let read = sundial::ui::model_preview::PackageRead::start();
            std::thread::spawn(move || {
                let _read = read;
                // One opening of the packages serves the layers and the art.
                let layers = open_shadowkeep_package_manager(&packages).and_then(|manager| {
                    let layers = IconLayers::from_manager(
                        &manager,
                        container_tag,
                        rarity,
                        corner.as_ref(),
                        crate::branding::Branding::for_packages(&packages),
                        plain,
                    )?;
                    let art = art.map(|art| art.read(&manager)).transpose()?;
                    Ok((layers, art))
                });
                let _ = sender.send(layers);
            });
            self.authored_icon_loading = Some((key.clone(), receiver));
        }
        context.request_repaint_after(std::time::Duration::from_millis(100));
        None
    }

    pub(super) fn authored_icon_rarity(&self) -> Option<crate::AuthoredWeaponRarity> {
        let hash = self.recipe.donor.item_hash.parse_u32().ok();
        effective_icon_rarity(
            self.recipe.overrides.rarity,
            (if self.recipe.kind.is_weapon() {
                self.donor_summaries.as_slice()
            } else {
                self.gear_donors_for(self.recipe.kind)
            })
            .iter()
            .find(|donor| Some(donor.hash) == hash)
            .map(|donor| donor.rarity),
        )
    }
}
