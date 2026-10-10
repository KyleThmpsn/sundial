use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use crate::package_runtime::reader::PackageManager;
use tiger_pkg::TagHash;

use crate::{
    catalog::icons::schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_FOREGROUND_LAYER_OFFSET, ICON_PRIMARY_LAYER_OFFSET,
        ICON_WATERMARK_LAYER_OFFSET,
    },
    image_processing::{blend_rgba_pixel, decode_bc1},
    investment::schema::{
        GLOBALS_ITEM_ICON_TABLE_SLOT, ITEM_ICON_CONTAINER_OFFSET, ITEM_ICON_ROW_CLASS,
        ITEM_ICON_ROW_SIZE, ITEM_STRING_ICON_INDEX_OFFSET, ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET,
        investment_globals_table_tag,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at, u32_at},
    package_runtime,
};

use super::Catalog;

const CATALOG_ICON_SIZE: usize = 96;
const MAX_CACHED_CATALOG_ICONS: usize = 512;
const MAX_PENDING_CATALOG_ICONS: usize = 64;
/// The most the cached textures take together. An icon is small, but a texture preview can be
/// 1024 pixels square, and the icon count alone would let 2 GiB of those stay loaded.
const MAX_CACHED_ICON_BYTES: usize = 256 << 20;
const FAILED_ICON_RETRY_DELAY: Duration = Duration::from_secs(5);
const STAT_ICON_CACHE_PREFIX: u64 = 1_u64 << 63;
const TEXTURE_ICON_CACHE_PREFIX: u64 = 1_u64 << 61;
/// Namespaces subclass node icons, which are keyed by their container tag.
const SUBCLASS_ICON_CACHE_PREFIX: u64 = 1_u64 << 60;
/// Namespaces an item's second icon, such as an emblem's nameplate, keyed by its container tag.
const SECONDARY_ICON_CACHE_PREFIX: u64 = 1_u64 << 59;
/// Namespaces the ammunition marks, keyed by their container tag.
const AMMO_ICON_CACHE_PREFIX: u64 = 1_u64 << 58;

#[cfg(test)]
mod queue_tests;
pub(crate) mod schema;

#[derive(Default)]
pub(super) struct IconRuntime {
    textures: HashMap<u64, (CachedIcon, u64)>,
    pending: HashSet<u64>,
    worker: Option<IconWorker>,
    access_counter: u64,
    suspended: bool,
}

struct IconWorker {
    requests: Option<SyncSender<IconLoadRequest>>,
    results: Receiver<IconLoadResult>,
    thread: Option<thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct IconLoadRequest {
    hash: u64,
    container: u32,
    native_size: bool,
    layers: IconLayers,
}

/// Which layers of an icon container a request composites.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IconLayers {
    /// Everything the client draws, including the season watermark and any foreground overlay.
    All,
    Texture,
    /// One layer alone at its own size, by its container offset: an emblem nameplate's banner,
    /// overlay or wide background, which a full composite would scale into one image.
    Layer(usize),
}

struct IconLoadResult {
    hash: u64,
    loaded: Result<LoadedCatalogIcon, String>,
}

impl Catalog {
    /// Stops icon and inspector work and waits until their package files have been released.
    pub(crate) fn suspend_package_access(&self) {
        self.inspection_access.suspend();
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.suspend();
    }

    /// Allows package-backed icon work to start again after an authoring session.
    pub(crate) fn resume_package_access(&self) {
        self.inspection_access.resume();
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.resume();
    }

    /// Loads an installed package icon on demand and keeps only displayed icons on the GPU.
    pub(crate) fn icon_texture(
        &self,
        context: &eframe::egui::Context,
        hash: u64,
    ) -> Option<eframe::egui::TextureHandle> {
        let &container = self.icon_containers.get(&hash)?;
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture(context, &self.install_path, hash, container)
    }

    pub(crate) fn texture_icon(
        &self,
        context: &eframe::egui::Context,
        tag: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture_with(
            context,
            &self.install_path,
            TEXTURE_ICON_CACHE_PREFIX | u64::from(tag),
            tag,
            true,
            IconLayers::Texture,
        )
    }

    pub(crate) fn icon_texture_from_container(
        &self,
        context: &eframe::egui::Context,
        cache_key: u64,
        container: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture(context, &self.install_path, cache_key, container)
    }

    /// The game's mark for an ammunition type, which carries its own class colour.
    pub(crate) fn ammo_icon_texture(
        &self,
        context: &eframe::egui::Context,
        ammo: super::ItemWeaponAmmoType,
    ) -> Option<eframe::egui::TextureHandle> {
        let index = match ammo {
            super::ItemWeaponAmmoType::Primary => 0,
            super::ItemWeaponAmmoType::Special => 1,
            super::ItemWeaponAmmoType::Heavy => 2,
        };
        let container = self.ammo_icon_containers[index]?;
        self.icon_texture_from_container(
            context,
            AMMO_ICON_CACHE_PREFIX | u64::from(container),
            container,
        )
    }

    /// A subclass node's icon, from the container its display record names.
    pub(crate) fn subclass_icon_texture(
        &self,
        context: &eframe::egui::Context,
        container: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        self.icon_texture_from_container(
            context,
            SUBCLASS_ICON_CACHE_PREFIX | u64::from(container),
            container,
        )
    }

    /// The container an item's second icon row names: an emblem's nameplate.
    pub(crate) fn secondary_icon_container(&self, hash: u64) -> Option<u32> {
        self.item_package_metadata
            .get(&hash)?
            .secondary_icon_container_tag
    }

    /// One layer of an item's second icon at its own size: an emblem nameplate's 474x96 banner
    /// (+0x14), its overlay (+0x20) or its wide background (+0x24).
    pub(crate) fn secondary_icon_texture(
        &self,
        context: &eframe::egui::Context,
        hash: u64,
        layer_offset: usize,
    ) -> Option<eframe::egui::TextureHandle> {
        let container = self.secondary_icon_container(hash)?;
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture_with(
            context,
            &self.install_path,
            SECONDARY_ICON_CACHE_PREFIX | ((layer_offset as u64) << 32) | u64::from(container),
            container,
            true,
            IconLayers::Layer(layer_offset),
        )
    }

    /// Preserves the primary layer's dimensions for artwork such as portrait badges.
    pub(crate) fn icon_texture_with_native_size(
        &self,
        context: &eframe::egui::Context,
        cache_key: u64,
        container: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture_with_size(context, &self.install_path, cache_key, container, true)
    }

    /// Loads the icon authored on an installed investment-stat definition.
    pub(crate) fn armor_stat_icon_texture(
        &self,
        context: &eframe::egui::Context,
        stat_name: &str,
    ) -> Option<eframe::egui::TextureHandle> {
        let definition = self
            .item_stat_definitions
            .iter()
            .find(|definition| definition.name.trim().eq_ignore_ascii_case(stat_name))?;
        let container = definition.icon_container?;
        let cache_key = STAT_ICON_CACHE_PREFIX | definition.hash;
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture(context, &self.install_path, cache_key, container)
    }

    pub(crate) fn icon_diagnostic(&self, hash: u64) -> Option<String> {
        let runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.diagnostic(hash)
    }
}

pub(crate) fn scan_item_icon_containers(
    manager: &PackageManager,
    globals: &[u8],
) -> Result<Vec<Option<u32>>, String> {
    let table_tag = TagHash(investment_globals_table_tag(
        globals,
        GLOBALS_ITEM_ICON_TABLE_SLOT,
    )?);
    let table = manager
        .read_tag(table_tag)
        .map_err(|error| format!("Could not read item icon table: {error}"))?;
    let (count, rows, class) = array_at(&table, 8)?;
    if class != ITEM_ICON_ROW_CLASS || count > usize::from(u16::MAX) {
        return Err(format!(
            "Item icon table has incompatible layout ({count} rows, class 0x{class:08X})"
        ));
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(
                    index
                        .checked_mul(ITEM_ICON_ROW_SIZE)
                        .ok_or("Item icon table offset overflowed")?,
                )
                .ok_or("Item icon table offset overflowed")?;
            let tag = u32_at(&table, row + ITEM_ICON_CONTAINER_OFFSET)?;
            if tag == u32::MAX {
                return Ok(None);
            }
            let tag_hash = TagHash(tag);
            if !package_runtime::is_valid_package_tag(tag_hash) {
                return Err(format!(
                    "Item icon row {index} has malformed container reference 0x{tag:08X}; absent references must be 0xFFFFFFFF"
                ));
            }
            if manager.get_entry(tag_hash).is_none() {
                return Err(format!(
                    "Item icon row {index} references unavailable container tag {tag_hash}"
                ));
            }
            Ok(Some(tag))
        })
        .collect()
}

pub(super) fn item_icon_container(
    item_strings: &[u8],
    containers_by_index: &[Option<u32>],
) -> Option<u32> {
    icon_container_at(
        item_strings,
        ITEM_STRING_ICON_INDEX_OFFSET,
        containers_by_index,
    )
}

/// The container an item's second icon row names: an emblem's nameplate.
pub(super) fn item_secondary_icon_container(
    item_strings: &[u8],
    containers_by_index: &[Option<u32>],
) -> Option<u32> {
    icon_container_at(
        item_strings,
        ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET,
        containers_by_index,
    )
}

fn icon_container_at(
    item_strings: &[u8],
    offset: usize,
    containers_by_index: &[Option<u32>],
) -> Option<u32> {
    let index = u16_at(item_strings, offset).ok()?;
    (index != u16::MAX)
        .then(|| {
            containers_by_index
                .get(usize::from(index))
                .copied()
                .flatten()
        })
        .flatten()
}

impl IconRuntime {
    pub(super) fn texture(
        &mut self,
        context: &eframe::egui::Context,
        install_path: &Path,
        hash: u64,
        container: u32,
    ) -> Option<eframe::egui::TextureHandle> {
        self.texture_with_size(context, install_path, hash, container, false)
    }

    fn texture_with_size(
        &mut self,
        context: &eframe::egui::Context,
        install_path: &Path,
        hash: u64,
        container: u32,
        native_size: bool,
    ) -> Option<eframe::egui::TextureHandle> {
        self.texture_with(
            context,
            install_path,
            hash,
            container,
            native_size,
            IconLayers::All,
        )
    }

    fn texture_with(
        &mut self,
        context: &eframe::egui::Context,
        install_path: &Path,
        hash: u64,
        container: u32,
        native_size: bool,
        layers: IconLayers,
    ) -> Option<eframe::egui::TextureHandle> {
        self.install_completed(context);
        self.access_counter = self.access_counter.wrapping_add(1);
        let access = self.access_counter;
        let now = Instant::now();
        if let Some((cached, last_access)) = self.textures.get_mut(&hash) {
            *last_access = access;
            match cached {
                CachedIcon::Loaded { texture, .. } => return Some(texture.clone()),
                CachedIcon::Failed { retry_after, .. } if now < *retry_after => return None,
                CachedIcon::Failed { .. } => {}
            }
        }
        self.textures.remove(&hash);
        if self.pending.contains(&hash) || self.pending.len() >= MAX_PENDING_CATALOG_ICONS {
            return None;
        }
        if self.suspended {
            return None;
        }
        if self.worker.is_none() {
            match IconWorker::spawn(install_path.to_owned(), context.clone()) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    self.cache(
                        hash,
                        CachedIcon::Failed {
                            error,
                            retry_after: now + FAILED_ICON_RETRY_DELAY,
                        },
                        access,
                    );
                    return None;
                }
            }
        }
        let request = IconLoadRequest {
            hash,
            container,
            native_size,
            layers,
        };
        let queued = self
            .worker
            .as_ref()
            .and_then(|worker| worker.requests.as_ref())
            .map(|requests| requests.try_send(request));
        match queued {
            Some(Ok(())) => {
                self.pending.insert(hash);
            }
            Some(Err(TrySendError::Full(_))) => {
                context.request_repaint_after(Duration::from_millis(50));
            }
            Some(Err(TrySendError::Disconnected(_))) | None => {
                self.fail_worker("The package icon loader stopped unexpectedly");
                self.access_counter = self.access_counter.wrapping_add(1);
                let access = self.access_counter;
                self.cache(
                    hash,
                    CachedIcon::Failed {
                        error: "The package icon loader stopped unexpectedly".to_owned(),
                        retry_after: now + FAILED_ICON_RETRY_DELAY,
                    },
                    access,
                );
            }
        }
        None
    }

    pub(super) fn diagnostic(&self, hash: u64) -> Option<String> {
        let (cached, _) = self.textures.get(&hash)?;
        match cached {
            CachedIcon::Loaded { warnings, .. } if !warnings.is_empty() => {
                Some(warnings.join("; "))
            }
            CachedIcon::Failed { error, .. } => Some(error.clone()),
            CachedIcon::Loaded { .. } => None,
        }
    }

    /// Keeps `icon`, first dropping the least recently drawn ones until both the count and the
    /// bytes leave room for it.
    fn cache(&mut self, hash: u64, icon: CachedIcon, access: u64) {
        let incoming = icon.bytes();
        while (self.textures.len() >= MAX_CACHED_CATALOG_ICONS
            || self.cached_bytes() + incoming > MAX_CACHED_ICON_BYTES)
            && let Some(oldest) = self
                .textures
                .iter()
                .min_by_key(|(_, (_, last_access))| *last_access)
                .map(|(hash, _)| *hash)
        {
            self.textures.remove(&oldest);
        }
        self.textures.insert(hash, (icon, access));
    }

    fn cached_bytes(&self) -> usize {
        self.textures.values().map(|(icon, _)| icon.bytes()).sum()
    }

    fn install_completed(&mut self, context: &eframe::egui::Context) {
        // Upload as results arrive, keeping decoded images out of an unbounded
        // intermediate vector while the worker continues producing them.
        for _ in 0..8 {
            let result = match self.worker.as_ref().map(|worker| worker.results.try_recv()) {
                Some(Ok(result)) => result,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.fail_worker("The package icon loader stopped unexpectedly");
                    break;
                }
            };
            self.pending.remove(&result.hash);
            self.access_counter = self.access_counter.wrapping_add(1);
            let access = self.access_counter;
            let cached = match result.loaded {
                Ok(loaded) => CachedIcon::Loaded {
                    texture: context.load_texture(
                        format!("catalog-icon-{:08X}", result.hash),
                        loaded.image,
                        eframe::egui::TextureOptions::LINEAR,
                    ),
                    warnings: loaded.warnings,
                },
                Err(error) => CachedIcon::Failed {
                    error,
                    retry_after: Instant::now() + FAILED_ICON_RETRY_DELAY,
                },
            };
            self.cache(result.hash, cached, access);
        }
    }

    fn fail_worker(&mut self, fallback_error: &str) {
        let mut error = fallback_error.to_owned();
        if let Some(mut worker) = self.worker.take()
            && let Err(join_error) = worker.shutdown()
        {
            error = join_error;
        }
        let retry_after = Instant::now() + FAILED_ICON_RETRY_DELAY;
        for hash in std::mem::take(&mut self.pending) {
            self.access_counter = self.access_counter.wrapping_add(1);
            self.cache(
                hash,
                CachedIcon::Failed {
                    error: error.clone(),
                    retry_after,
                },
                self.access_counter,
            );
        }
    }

    fn suspend(&mut self) {
        self.suspended = true;
        self.pending.clear();
        if let Some(mut worker) = self.worker.take() {
            drop(worker.shutdown());
        }
    }

    fn resume(&mut self) {
        self.suspended = false;
    }
}

impl IconWorker {
    fn spawn(install_path: PathBuf, context: eframe::egui::Context) -> Result<Self, String> {
        let (request_sender, request_receiver) =
            mpsc::sync_channel::<IconLoadRequest>(MAX_PENDING_CATALOG_ICONS);
        let (result_sender, result_receiver) = mpsc::sync_channel::<IconLoadResult>(2);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let thread = thread::Builder::new()
            .name("sundial-icon-loader".to_owned())
            .spawn(move || {
                run_icon_worker(
                    &install_path,
                    &context,
                    request_receiver,
                    result_sender,
                    &worker_cancel,
                );
            })
            .map_err(|error| format!("Could not start the package icon loader: {error}"))?;
        Ok(Self {
            requests: Some(request_sender),
            results: result_receiver,
            thread: Some(thread),
            cancel,
        })
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.cancel.store(true, Ordering::Relaxed);
        self.requests.take();
        // A bounded result queue may have a blocked sender. Release its receiver
        // before joining, then wait only for the active read to release packages.
        self.results = mpsc::channel().1;
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "The package icon loader panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for IconWorker {
    fn drop(&mut self) {
        drop(self.shutdown());
    }
}

fn run_icon_worker(
    install_path: &Path,
    context: &eframe::egui::Context,
    requests: Receiver<IconLoadRequest>,
    results: SyncSender<IconLoadResult>,
    cancel: &AtomicBool,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let manager = package_runtime::open_shadowkeep_packages(install_path)
        .map_err(|error| format!("Could not open the installed packages: {error}"));
    run_icon_requests(
        context,
        requests,
        results,
        cancel,
        |request| match &manager {
            Ok(manager) => load_catalog_icon(
                manager,
                TagHash(request.container),
                request.native_size,
                request.layers,
            ),
            Err(error) => Err(error.clone()),
        },
    );
}

fn run_icon_requests(
    context: &eframe::egui::Context,
    requests: Receiver<IconLoadRequest>,
    results: SyncSender<IconLoadResult>,
    cancel: &AtomicBool,
    mut load: impl FnMut(&IconLoadRequest) -> Result<LoadedCatalogIcon, String>,
) {
    while let Ok(request) = requests.recv() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let loaded = load(&request);
        if results
            .send(IconLoadResult {
                hash: request.hash,
                loaded,
            })
            .is_err()
        {
            break;
        }
        context.request_repaint();
    }
}

enum CachedIcon {
    Loaded {
        texture: eframe::egui::TextureHandle,
        warnings: Vec<String>,
    },
    Failed {
        error: String,
        retry_after: Instant,
    },
}

impl CachedIcon {
    /// What the texture takes on the graphics card, four bytes a pixel.
    fn bytes(&self) -> usize {
        match self {
            Self::Loaded { texture, .. } => {
                let [width, height] = texture.size();
                width * height * 4
            }
            Self::Failed { .. } => 0,
        }
    }
}

struct LoadedCatalogIcon {
    image: eframe::egui::ColorImage,
    warnings: Vec<String>,
}

fn load_catalog_icon(
    manager: &PackageManager,
    container_tag: TagHash,
    native_size: bool,
    layers: IconLayers,
) -> Result<LoadedCatalogIcon, String> {
    if layers == IconLayers::Texture {
        return load_texture_icon(manager, container_tag).map(|image| LoadedCatalogIcon {
            image,
            warnings: Vec::new(),
        });
    }
    let container = manager
        .read_tag(container_tag)
        .map_err(|error| format!("Could not read icon container: {error}"))?;
    if let IconLayers::Layer(offset) = layers {
        return Ok(LoadedCatalogIcon {
            image: load_catalog_icon_layer(manager, &container, offset)?
                .ok_or("Icon container has no texture in that layer")?,
            warnings: Vec::new(),
        });
    }
    let mut warnings = Vec::new();
    let background = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_BACKGROUND_LAYER_OFFSET,
        "background",
        &mut warnings,
    );
    let background_overlay = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_WATERMARK_LAYER_OFFSET,
        "watermark",
        &mut warnings,
    );
    let primary = load_catalog_icon_layer(manager, &container, ICON_PRIMARY_LAYER_OFFSET)?
        .ok_or("Item icon has no primary texture")?;
    let size = if native_size {
        primary.size
    } else {
        [CATALOG_ICON_SIZE; 2]
    };
    let overlay = load_optional_catalog_icon_layer(
        manager,
        &container,
        ICON_FOREGROUND_LAYER_OFFSET,
        "foreground overlay",
        &mut warnings,
    );
    Ok(LoadedCatalogIcon {
        image: composite_catalog_icon_at_size(
            [background, Some(primary), background_overlay, overlay]
                .into_iter()
                .flatten(),
            size,
        ),
        warnings,
    })
}

fn load_optional_catalog_icon_layer(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<eframe::egui::ColorImage> {
    match load_catalog_icon_layer(manager, icon_container, layer_offset) {
        Ok(layer) => layer,
        Err(error) => {
            warnings.push(format!("Could not load icon {label}: {error}"));
            None
        }
    }
}

fn load_catalog_icon_layer(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
) -> Result<Option<eframe::egui::ColorImage>, String> {
    load_catalog_icon_layer_at(manager, icon_container, layer_offset, 0, 0)
}

fn load_catalog_icon_layer_at(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
    lane_index: usize,
    texture_index: usize,
) -> Result<Option<eframe::egui::ColorImage>, String> {
    let layer_tag = TagHash(u32_at(icon_container, layer_offset)?);
    if !package_runtime::is_valid_package_tag(layer_tag) {
        return Ok(None);
    }
    let layer = manager
        .read_tag(layer_tag)
        .map_err(|error| format!("Could not read icon layer container: {error}"))?;
    let resource = relative_offset(0x10, 0, i64_at(&layer, 0x10)?)?;
    let (lane_count, lanes, _) = array_at(&layer, resource)?;
    if lane_index >= lane_count {
        return Ok(None);
    }
    let lane = lanes
        .checked_add(
            lane_index
                .checked_mul(0x10)
                .ok_or("Icon texture lane offset overflowed")?,
        )
        .ok_or("Icon texture lane offset overflowed")?;
    let (texture_count, textures, _) = array_at(&layer, lane)?;
    if texture_index >= texture_count {
        return Ok(None);
    }
    let texture = textures
        .checked_add(
            texture_index
                .checked_mul(4)
                .ok_or("Icon texture offset overflowed")?,
        )
        .ok_or("Icon texture offset overflowed")?;
    let texture_tag = TagHash(u32_at(&layer, texture)?);
    load_texture_icon(manager, texture_tag).map(Some)
}

fn load_texture_icon(
    manager: &PackageManager,
    texture_tag: TagHash,
) -> Result<eframe::egui::ColorImage, String> {
    let header = manager
        .read_tag(texture_tag)
        .map_err(|error| format!("Could not read icon layer texture header: {error}"))?;
    let entry = manager
        .get_entry(texture_tag)
        .ok_or("Icon layer texture is missing from the package index")?;
    let data_tag = TagHash(entry.reference);
    if !package_runtime::is_valid_package_tag(data_tag) {
        return Err("Icon layer texture has no data resource".into());
    }
    let data = manager
        .read_tag(data_tag)
        .map_err(|error| format!("Could not read icon layer texture: {error}"))?;
    decode_catalog_texture(&header, &data)
}

#[cfg(test)]
fn composite_catalog_icon(
    layers: impl IntoIterator<Item = eframe::egui::ColorImage>,
) -> eframe::egui::ColorImage {
    composite_catalog_icon_at_size(layers, [CATALOG_ICON_SIZE; 2])
}

fn composite_catalog_icon_at_size(
    layers: impl IntoIterator<Item = eframe::egui::ColorImage>,
    [width, height]: [usize; 2],
) -> eframe::egui::ColorImage {
    let mut rgba = vec![0_u8; width * height * 4];
    for layer in layers {
        let [source_width, source_height] = layer.size;
        if source_width == 0 || source_height == 0 {
            continue;
        }
        for y in 0..height {
            let source_y = y * source_height / height;
            for x in 0..width {
                let source_x = x * source_width / width;
                let source =
                    layer.pixels[source_y * source_width + source_x].to_srgba_unmultiplied();
                let destination_offset = (y * width + x) * 4;
                blend_rgba_pixel(
                    &mut rgba[destination_offset..destination_offset + 4],
                    source,
                );
            }
        }
    }
    eframe::egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba)
}

/// Decodes one texture.
fn decode_catalog_texture(header: &[u8], data: &[u8]) -> Result<eframe::egui::ColorImage, String> {
    let format = u32_at(header, 4)?;
    let width = usize::from(u16_at(header, 0x0E)?);
    let height = usize::from(u16_at(header, 0x10)?);
    // Emblem nameplate backgrounds run to 2300 wide.
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err("Item icon texture dimensions are invalid".into());
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or("Item icon texture dimensions overflowed")?;
    let rgba = match format {
        // DXGI_FORMAT_R8G8B8A8_UNORM and _SRGB.
        28 | 29 => {
            let length = pixel_count
                .checked_mul(4)
                .ok_or("Item icon texture size overflowed")?;
            data.get(..length)
                .ok_or("Item icon texture data is truncated")?
                .to_vec()
        }
        // DXGI_FORMAT_BC1_UNORM and _SRGB.
        71 | 72 => decode_bc1(data, width, height)?,
        _ => return Err(format!("Unsupported item icon texture format {format}")),
    };
    Ok(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        &rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_icon_worker_releases_pending_hashes() {
        let (request_sender, _request_receiver) = mpsc::sync_channel(MAX_PENDING_CATALOG_ICONS);
        let (result_sender, result_receiver) = mpsc::channel();
        drop(result_sender);
        let hash = 0x1234_5678;
        let mut runtime = IconRuntime {
            pending: HashSet::from([hash]),
            worker: Some(IconWorker {
                requests: Some(request_sender),
                results: result_receiver,
                thread: None,
                cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }),
            ..IconRuntime::default()
        };

        runtime.install_completed(&eframe::egui::Context::default());

        assert!(runtime.worker.is_none());
        assert!(runtime.pending.is_empty());
        assert!(
            runtime
                .diagnostic(hash)
                .is_some_and(|message| message.contains("stopped"))
        );
    }

    #[test]
    fn rgba_catalog_texture_decodes_package_pixels() {
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&28_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&2_u16.to_le_bytes());
        header[0x10..0x12].copy_from_slice(&1_u16.to_le_bytes());
        let image = decode_catalog_texture(&header, &[255, 0, 0, 255, 0, 255, 0, 128]).unwrap();

        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            [
                eframe::egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255),
                eframe::egui::Color32::from_rgba_unmultiplied(0, 255, 0, 128),
            ]
        );
    }

    #[test]
    fn catalog_icon_layers_composite_in_package_display_order() {
        let background =
            eframe::egui::ColorImage::filled([1, 1], eframe::egui::Color32::from_rgb(255, 0, 0));
        let overlay = eframe::egui::ColorImage::filled(
            [1, 1],
            eframe::egui::Color32::from_rgba_unmultiplied(0, 0, 255, 128),
        );
        let image = composite_catalog_icon([background, overlay]);

        assert_eq!(image.size, [CATALOG_ICON_SIZE, CATALOG_ICON_SIZE]);
        assert!(image.pixels.iter().all(|pixel| {
            *pixel == eframe::egui::Color32::from_rgba_unmultiplied(127, 0, 128, 255)
        }));
    }
}
