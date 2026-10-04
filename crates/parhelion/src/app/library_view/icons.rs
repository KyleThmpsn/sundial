//! Authored icon previews requested by visible library and build-selection rows.
use super::*;

const RETAINED: usize = 128;

#[derive(Default)]
pub(in crate::app) struct Icons {
    pub(super) previews: BTreeMap<PathBuf, AuthoredIconPreview<IconKey>>,
    receiver: Option<Receiver<LibraryIconResult>>,
    pub(super) worker: Option<thread::JoinHandle<()>>,
    pending: BTreeMap<PathBuf, IconKey>,
    pub(super) wanted: BTreeSet<PathBuf>,
    unavailable: BTreeSet<PathBuf>,
    access: BTreeMap<PathBuf, u64>,
    clock: u64,
}

impl Drop for Icons {
    fn drop(&mut self) {
        // Installation and catalog replacement require every package reader released.
        self.receiver = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Icons {
    pub(super) fn want(&mut self, ctx: &egui::Context, path: &Path) {
        self.wanted.insert(path.to_owned());
        self.touch(path);
        if !self.previews.contains_key(path)
            && !self.pending.contains_key(path)
            && !self.unavailable.contains(path)
        {
            ctx.request_repaint();
        }
    }

    fn touch(&mut self, path: &Path) {
        self.clock += 1;
        self.access.insert(path.to_owned(), self.clock);
    }

    fn retain(&mut self) {
        while self.previews.len() > RETAINED {
            let oldest = self
                .previews
                .keys()
                .min_by_key(|path| self.access.get(*path).copied().unwrap_or(0))
                .cloned();
            let Some(oldest) = oldest else { break };
            self.previews.remove(&oldest);
            self.access.remove(&oldest);
        }
        // Requests for offscreen recipes must not create another unbounded metadata cache.
        self.access.retain(|path, _| {
            self.previews.contains_key(path)
                || self.pending.contains_key(path)
                || self.wanted.contains(path)
        });
    }

    fn insert(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        key: IconKey,
        result: Result<egui::ColorImage, String>,
    ) {
        let preview = match result {
            Ok(image) => AuthoredIconPreview::Ready {
                texture: ctx.load_texture(
                    format!("library-icon-{}", path.display()),
                    image,
                    egui::TextureOptions::LINEAR,
                ),
                key,
            },
            Err(error) => AuthoredIconPreview::Failed { key, error },
        };
        self.touch(&path);
        self.previews.insert(path, preview);
        self.retain();
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        let finished = self
            .worker
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished);
        if finished {
            let _ = self.worker.take().unwrap().join();
        }
        // End the receiver borrow before inserting and evicting texture handles.
        loop {
            let Some((path, key, result)) = self
                .receiver
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            else {
                break;
            };
            if self.pending.remove(&path).as_ref() == Some(&key) {
                self.insert(ctx, path, key, result);
            }
        }
        if finished {
            self.receiver = None;
            for (path, key) in std::mem::take(&mut self.pending) {
                self.insert(
                    ctx,
                    path,
                    key,
                    Err("Library icon loading stopped unexpectedly".into()),
                );
            }
        }
    }

    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        catalog: &InvestmentCatalog,
        donors: &[WeaponDonorSummary],
        entries: &[RecipeLibraryEntry],
    ) {
        self.poll(ctx);
        let wanted = std::mem::take(&mut self.wanted);
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let paths: BTreeSet<_> = entries.iter().map(|entry| &entry.path).collect();
        self.previews.retain(|path, _| paths.contains(path));
        self.unavailable.retain(|path| wanted.contains(path));
        self.retain();
        let missing = entries
            .iter()
            .filter(|entry| wanted.contains(&entry.path))
            .filter_map(|entry| {
                let Some(container_tag) = catalog.weapon_icon_container(entry.icon_hash) else {
                    self.unavailable.insert(entry.path.clone());
                    return None;
                };
                let Some(rarity) = effective_icon_rarity(
                    entry.rarity,
                    donors
                        .iter()
                        .find(|donor| donor.hash == entry.donor_hash)
                        .map(|donor| donor.rarity),
                ) else {
                    self.unavailable.insert(entry.path.clone());
                    return None;
                };
                self.unavailable.remove(&entry.path);
                let key = IconKey {
                    corner_icon: entry.corner_icon,
                    item_hash: entry.icon_hash,
                    container_tag,
                    rarity,
                    edit: entry.icon_edit.clone(),
                    plain: entry.kind == crate::ItemKind::Subclass,
                };
                let cached = self.previews.get(&entry.path).map(|preview| match preview {
                    AuthoredIconPreview::Ready { key, .. }
                    | AuthoredIconPreview::Failed { key, .. } => key,
                });
                (cached != Some(&key)).then(|| (entry.path.clone(), key))
            })
            .take(RETAINED)
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return;
        }
        let packages = packages.to_owned();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.pending = missing.iter().cloned().collect();
        self.worker = Some(thread::spawn(move || {
            let manager = open_shadowkeep_package_manager(&packages);
            let branding = crate::branding::Branding::for_packages(&packages);
            for (path, key) in missing {
                let corner = match key.corner_icon {
                    Some(_) => crate::recipe_library::load_corner_icon(&path),
                    None => Ok(None),
                };
                let result = corner.and_then(|corner| {
                    crate::icon_edit::render_weapon_icon_preview_from_manager(
                        manager.as_ref().map_err(Clone::clone)?,
                        TagHash(key.container_tag),
                        key.rarity,
                        &key.edit,
                        corner.as_ref(),
                        branding,
                        key.plain,
                    )
                });
                if sender.send((path, key, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            ctx.request_repaint();
        }));
    }
}

#[cfg(test)]
mod tests;
