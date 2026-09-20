use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use tiger_pkg::{PackageManager, TagHash};

use crate::{
    icon_schema::{
        ICON_BACKGROUND_LAYER_OFFSET, ICON_FOREGROUND_LAYER_OFFSET, ICON_PRIMARY_LAYER_OFFSET,
        ICON_WATERMARK_LAYER_OFFSET,
    },
    image_processing::{blend_rgba_pixel, clear_color_region, decode_bc1},
    investment_schema::{
        GLOBALS_ITEM_ICON_TABLE_SLOT, ITEM_ICON_CONTAINER_OFFSET, ITEM_ICON_ROW_CLASS,
        ITEM_ICON_ROW_SIZE, ITEM_STRING_ICON_INDEX_OFFSET, investment_globals_table_tag,
    },
    package_payload::{array_at, i64_at, relative_offset, u16_at, u32_at},
    package_runtime,
};

use super::Catalog;

const CATALOG_ICON_SIZE: usize = 96;
const MAX_CACHED_CATALOG_ICONS: usize = 512;
const FAILED_ICON_RETRY_DELAY: Duration = Duration::from_secs(5);
const STAT_ICON_CACHE_PREFIX: u64 = 1_u64 << 63;
/// Namespaces the overlay-free copy of an item icon so both variants can stay cached.
const ARTWORK_ICON_CACHE_PREFIX: u64 = 1_u64 << 62;
const TEXTURE_ICON_CACHE_PREFIX: u64 = 1_u64 << 61;

/// The cache key of an artwork icon. The cleared color is part of the key: two requests for
/// one item with different cleared colors composite differently, so they must not read each
/// other's texture. Item hashes are 32-bit values widened on read, which the stat icon
/// prefix above already relies on, so the colour occupies bits 32 to 56.
fn artwork_cache_key(hash: u64, cleared_color: Option<[u8; 3]>) -> u64 {
    let cleared = cleared_color.map_or(0, |[red, green, blue]| {
        1 << 24 | u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue)
    });
    ARTWORK_ICON_CACHE_PREFIX | (u64::from(cleared) << 32) | (hash & u64::from(u32::MAX))
}

#[derive(Default)]
pub(super) struct IconRuntime {
    textures: HashMap<u64, (CachedIcon, u64)>,
    pending: HashSet<u64>,
    worker: Option<IconWorker>,
    access_counter: u64,
    suspended: bool,
}

struct IconWorker {
    requests: Option<Sender<IconLoadRequest>>,
    results: Receiver<IconLoadResult>,
    thread: Option<thread::JoinHandle<()>>,
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
    /// The artwork layer alone, without the rarity plate, watermark or foreground overlay, and
    /// with one flat color cleared from the artwork itself. Authoring controls use this to show
    /// an appearance rather than the stock item presentation around it, which an authored weapon
    /// replaces with its own.
    Artwork {
        cleared_color: Option<[u8; 3]>,
    },
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

    /// Loads an item icon as artwork alone, optionally clearing one flat color from it.
    pub(crate) fn icon_texture_artwork(
        &self,
        context: &eframe::egui::Context,
        hash: u64,
        cleared_color: Option<[u8; 3]>,
    ) -> Option<eframe::egui::TextureHandle> {
        let &container = self.icon_containers.get(&hash)?;
        let mut runtime = self
            .icon_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runtime.texture_with(
            context,
            &self.install_path,
            artwork_cache_key(hash, cleared_color),
            container,
            false,
            IconLayers::Artwork { cleared_color },
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
    let index = u16_at(item_strings, ITEM_STRING_ICON_INDEX_OFFSET).ok()?;
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
        if self.pending.contains(&hash) {
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
            .is_some_and(|requests| requests.send(request).is_ok());
        if queued {
            self.pending.insert(hash);
        } else {
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

    fn cache(&mut self, hash: u64, icon: CachedIcon, access: u64) {
        if self.textures.len() >= MAX_CACHED_CATALOG_ICONS
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

    fn install_completed(&mut self, context: &eframe::egui::Context) {
        let mut completed = Vec::new();
        let mut disconnected = false;
        if let Some(worker) = &self.worker {
            loop {
                match worker.results.try_recv() {
                    Ok(result) => completed.push(result),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for result in completed {
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
        if disconnected {
            self.fail_worker("The package icon loader stopped unexpectedly");
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
        let (request_sender, request_receiver) = mpsc::channel::<IconLoadRequest>();
        let (result_sender, result_receiver) = mpsc::channel::<IconLoadResult>();
        let thread = thread::Builder::new()
            .name("sundial-icon-loader".to_owned())
            .spawn(move || {
                run_icon_worker(&install_path, &context, request_receiver, result_sender);
            })
            .map_err(|error| format!("Could not start the package icon loader: {error}"))?;
        Ok(Self {
            requests: Some(request_sender),
            results: result_receiver,
            thread: Some(thread),
        })
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.requests.take();
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
    results: Sender<IconLoadResult>,
) {
    let manager = package_runtime::open_shadowkeep_packages(install_path)
        .map_err(|error| format!("Could not open the installed packages: {error}"));
    while let Ok(request) = requests.recv() {
        let loaded = match &manager {
            Ok(manager) => load_catalog_icon(
                manager,
                TagHash(request.container),
                request.native_size,
                request.layers,
            ),
            Err(error) => Err(error.clone()),
        };
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
    let mut warnings = Vec::new();
    // An artwork request draws the primary layer alone, so the other three are not read at
    // all. Reading one would cost a decode it discards, and would record a warning about a
    // layer this icon never draws.
    let overlays = layers == IconLayers::All;
    let background = overlays
        .then(|| {
            load_optional_catalog_icon_layer(
                manager,
                &container,
                ICON_BACKGROUND_LAYER_OFFSET,
                "background",
                &mut warnings,
            )
        })
        .flatten();
    let background_overlay = overlays
        .then(|| {
            load_optional_catalog_icon_layer(
                manager,
                &container,
                ICON_WATERMARK_LAYER_OFFSET,
                "watermark",
                &mut warnings,
            )
        })
        .flatten();
    let cleared = match layers {
        IconLayers::Artwork { cleared_color } => cleared_color,
        IconLayers::All | IconLayers::Texture => None,
    };
    let primary = load_catalog_icon_layer(manager, &container, ICON_PRIMARY_LAYER_OFFSET, cleared)?
        .ok_or("Item icon has no primary texture")?;
    let size = if native_size {
        primary.size
    } else {
        [CATALOG_ICON_SIZE; 2]
    };
    let overlay = overlays
        .then(|| {
            load_optional_catalog_icon_layer(
                manager,
                &container,
                ICON_FOREGROUND_LAYER_OFFSET,
                "foreground overlay",
                &mut warnings,
            )
        })
        .flatten();
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
    match load_catalog_icon_layer(manager, icon_container, layer_offset, None) {
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
    cleared: Option<[u8; 3]>,
) -> Result<Option<eframe::egui::ColorImage>, String> {
    load_catalog_icon_layer_at(manager, icon_container, layer_offset, 0, 0, cleared)
}

fn load_catalog_icon_layer_at(
    manager: &PackageManager,
    icon_container: &[u8],
    layer_offset: usize,
    lane_index: usize,
    texture_index: usize,
    cleared: Option<[u8; 3]>,
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
    load_texture_icon_with_color(manager, texture_tag, cleared).map(Some)
}

fn load_texture_icon(
    manager: &PackageManager,
    texture_tag: TagHash,
) -> Result<eframe::egui::ColorImage, String> {
    load_texture_icon_with_color(manager, texture_tag, None)
}

fn load_texture_icon_with_color(
    manager: &PackageManager,
    texture_tag: TagHash,
    cleared: Option<[u8; 3]>,
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
    decode_catalog_texture(&header, &data, cleared)
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

/// Decodes one texture, optionally clearing a flat color from it.
///
/// The clear runs here, on the unpremultiplied bytes the decoder produces, rather than on the
/// finished image: `ColorImage` stores premultiplied channels, so reading a color back out of
/// one is lossy, and the faintest plate pixels no longer match the color that identifies them.
fn decode_catalog_texture(
    header: &[u8],
    data: &[u8],
    cleared: Option<[u8; 3]>,
) -> Result<eframe::egui::ColorImage, String> {
    let format = u32_at(header, 4)?;
    let width = usize::from(u16_at(header, 0x0E)?);
    let height = usize::from(u16_at(header, 0x10)?);
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
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
    let mut rgba = rgba;
    if let Some(cleared) = cleared {
        clear_color_region(&mut rgba, width, height, cleared);
    }
    Ok(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        &rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cache entry per composited result. Two artwork requests for the same item with
    /// different cleared colors composite differently, so they must not share a key, and
    /// neither may collide with the item's own icon or with a stat icon.
    #[test]
    fn artwork_cache_keys_separate_cleared_colors_from_each_other_and_from_plain_icons() {
        let hash = 0x1234_5678_u64;
        let plate = Some([0xF2, 0xE3, 0x70]);
        let other = Some([0x8C, 0x45, 0xA7]);
        let keys = [
            artwork_cache_key(hash, plate),
            artwork_cache_key(hash, other),
            artwork_cache_key(hash, None),
            hash,
            STAT_ICON_CACHE_PREFIX | hash,
            TEXTURE_ICON_CACHE_PREFIX | hash,
        ];
        for (index, key) in keys.iter().enumerate() {
            for later in &keys[index + 1..] {
                assert_ne!(key, later, "cache keys collide: {keys:#X?}");
            }
        }
        // A black plate is a colour, not the absence of one.
        assert_ne!(
            artwork_cache_key(hash, Some([0, 0, 0])),
            artwork_cache_key(hash, None)
        );
        // The same request is the same entry.
        assert_eq!(
            artwork_cache_key(hash, plate),
            artwork_cache_key(hash, plate)
        );
    }

    #[test]
    fn disconnected_icon_worker_releases_pending_hashes() {
        let (request_sender, _request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        drop(result_sender);
        let hash = 0x1234_5678;
        let mut runtime = IconRuntime {
            pending: HashSet::from([hash]),
            worker: Some(IconWorker {
                requests: Some(request_sender),
                results: result_receiver,
                thread: None,
            }),
            ..IconRuntime::default()
        };

        runtime.install_completed(&eframe::egui::Context::default());

        assert!(runtime.worker.is_none());
        assert!(runtime.pending.is_empty());
        assert_eq!(
            runtime.diagnostic(hash).as_deref(),
            Some("The package icon loader stopped unexpectedly")
        );
    }

    #[test]
    fn rgba_catalog_texture_decodes_package_pixels() {
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&28_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&2_u16.to_le_bytes());
        header[0x10..0x12].copy_from_slice(&1_u16.to_le_bytes());
        let image =
            decode_catalog_texture(&header, &[255, 0, 0, 255, 0, 255, 0, 128], None).unwrap();

        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            [
                eframe::egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255),
                eframe::egui::Color32::from_rgba_unmultiplied(0, 255, 0, 128),
            ]
        );
    }

    /// The plate is painted at a range of alpha values, and clearing runs on the decoded
    /// bytes so every one of them still carries the exact color that identifies it. Reading
    /// the color back out of a finished `ColorImage` instead would lose the faintest pixels,
    /// which premultiplication cannot represent.
    #[test]
    fn clearing_reaches_plate_pixels_at_every_alpha() {
        let plate = [0xF2, 0xE3, 0x70];
        let alphas: [u8; 6] = [255, 128, 32, 16, 8, 2];
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&28_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&(alphas.len() as u16).to_le_bytes());
        header[0x10..0x12].copy_from_slice(&1_u16.to_le_bytes());
        let data: Vec<u8> = alphas
            .iter()
            .flat_map(|alpha| [plate[0], plate[1], plate[2], *alpha])
            .collect();

        let kept = decode_catalog_texture(&header, &data, None).unwrap();
        assert!(
            kept.pixels.iter().all(|pixel| pixel.a() > 0),
            "nothing should be cleared without a color"
        );

        let cleared = decode_catalog_texture(&header, &data, Some(plate)).unwrap();
        for (index, pixel) in cleared.pixels.iter().enumerate() {
            assert_eq!(
                *pixel,
                eframe::egui::Color32::TRANSPARENT,
                "plate pixel at alpha {} survived",
                alphas[index]
            );
        }
    }

    #[test]
    fn bc1_catalog_texture_decodes_package_blocks() {
        let mut header = vec![0_u8; 0x12];
        header[4..8].copy_from_slice(&71_u32.to_le_bytes());
        header[0x0E..0x10].copy_from_slice(&4_u16.to_le_bytes());
        header[0x10..0x12].copy_from_slice(&4_u16.to_le_bytes());
        let block = [0x00, 0xF8, 0xE0, 0x07, 0, 0, 0, 0];
        let image = decode_catalog_texture(&header, &block, None).unwrap();

        assert_eq!(image.size, [4, 4]);
        assert!(
            image
                .pixels
                .iter()
                .all(|pixel| *pixel == eframe::egui::Color32::RED)
        );
    }

    #[test]
    fn catalog_icon_layers_composite_in_package_display_order() {
        let background =
            eframe::egui::ColorImage::new([1, 1], eframe::egui::Color32::from_rgb(255, 0, 0));
        let overlay = eframe::egui::ColorImage::new(
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
