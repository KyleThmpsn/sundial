//! The shader page's dye materials and detail textures, read in the background and shared by its
//! surface tiles, its inspector, its icon and its texture thumbnails.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::*;
use crate::dye::write_vectors;
use crate::icon_edit::ImportedIcon;
use crate::shader_icon::{Finish, IconSurface, IconTexture, Iridescence as RampIridescence};
use sundial::package_authoring::{
    DyeFinish, DyeSurfaceMaterial, DyeTexture, dye_surfaces, load_dye_materials, load_dye_textures,
};

/// A dye's material as the page uses it: its vectors, and its detail textures' tags and tiling.
pub(super) struct Material {
    pub(super) vectors: [[f32; 4]; 27],
    pub(super) detail: Option<u32>,
    pub(super) normal: Option<u32>,
    pub(super) detail_tiling: [f32; 4],
    pub(super) normal_tiling: [f32; 4],
}

type Materials = BTreeMap<u16, Result<Material, String>>;
type Textures = BTreeMap<u32, Result<IconTexture, String>>;

/// The dyes and textures one background read loads.
type Request = (Vec<u16>, Vec<u32>);
#[cfg(feature = "d2-model-importer")]
type Sources = BTreeMap<i8, crate::shader::DyeSource>;
#[cfg(feature = "d2-model-importer")]
type SourceRead = Result<(Sources, Materials, Textures, SourceIdentity), String>;

#[cfg(feature = "d2-model-importer")]
pub(super) struct SourceIdentity {
    pub(super) name: Option<String>,
    pub(super) hash: Option<u32>,
    icon: Option<ImportedIcon>,
}

/// Dye materials by dye row and detail textures by tag, read in the background.
#[derive(Default)]
pub(in crate::app) struct DyeMaterials {
    #[cfg(feature = "d2-model-importer")]
    source_key: Option<String>,
    #[cfg(feature = "d2-model-importer")]
    source_job: Option<thread::JoinHandle<SourceRead>>,
    #[cfg(feature = "d2-model-importer")]
    pub(super) sources: BTreeMap<i8, crate::shader::DyeSource>,
    #[cfg(feature = "d2-model-importer")]
    pub(super) source_error: Option<String>,
    #[cfg(feature = "d2-model-importer")]
    pub(super) source: Option<SourceIdentity>,
    #[cfg(feature = "d2-model-importer")]
    source_icon: Option<egui::TextureHandle>,
    materials: Materials,
    textures: Textures,
    thumbnails: BTreeMap<u32, egui::TextureHandle>,
    job: Option<(Request, thread::JoinHandle<(Materials, Textures)>)>,
    /// The icon last drawn from the dyes, and a fingerprint of what it was drawn from.
    pub(super) drawn: Option<(u64, ImportedIcon)>,
    /// Each surface's ball, by channel then surface, and a fingerprint of what it was drawn from.
    swatches: [Option<(u64, egui::TextureHandle)>; 6],
}

impl Drop for DyeMaterials {
    fn drop(&mut self) {
        #[cfg(feature = "d2-model-importer")]
        if let Some(job) = self.source_job.take() {
            let _ = job.join();
        }
        // No package handles may outlive the catalog, as with the dye colors.
        if let Some((_, job)) = self.job.take() {
            let _ = job.join();
        }
    }
}

impl DyeMaterials {
    #[cfg(feature = "d2-model-importer")]
    pub(super) fn update_source(&mut self, ctx: &egui::Context, recipe: &WeaponRecipe) {
        let key = recipe
            .overrides
            .imported_graph
            .as_ref()
            .and_then(|g| serde_json::to_string(g).ok());
        if self.source_key != key {
            if let Some(job) = self.source_job.take() {
                let _ = job.join();
            }
            self.source_key = key.clone();
            self.sources.clear();
            self.source_error = None;
            self.source = None;
            self.source_icon = None;
            self.materials.clear();
            self.textures.clear();
            self.thumbnails.clear();
            self.swatches = Default::default();
            self.drawn = None;
            if key.is_some() {
                let recipe = recipe.clone();
                let ctx = ctx.clone();
                self.source_job = Some(thread::spawn(move || {
                    let result = read_source(&recipe);
                    ctx.request_repaint();
                    result
                }));
            }
        }
        if let Some(job) = self.source_job.take_if(|job| job.is_finished()) {
            match job
                .join()
                .unwrap_or_else(|_| Err("Source material loading stopped".into()))
            {
                Ok((sources, materials, textures, identity)) => {
                    self.source = Some(identity);
                    self.sources = sources;
                    self.materials.extend(materials);
                    self.textures.extend(textures);
                }
                Err(error) => self.source_error = Some(error),
            }
        }
    }

    #[cfg(feature = "d2-model-importer")]
    pub(super) fn source_icon(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if self.source_icon.is_none() {
            let icon = self.source.as_ref()?.icon.as_ref()?;
            let pixels = icon.fit_to(96, 96);
            self.source_icon = Some(ctx.load_texture(
                "parhelion-source-shader-icon",
                egui::ColorImage::from_rgba_unmultiplied([96, 96], pixels.as_raw()),
                egui::TextureOptions::LINEAR,
            ));
        }
        self.source_icon.clone()
    }

    /// Keeps the materials of `dyes` and the textures of `tags` loaded, reading any missing ones in
    /// the background. A kept dye's own textures stay too.
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        dyes: &BTreeSet<u16>,
        tags: &BTreeSet<u32>,
    ) {
        if let Some(((requested_dyes, requested_tags), job)) =
            self.job.take_if(|(_, job)| job.is_finished())
        {
            // A stopped read counts its requests as failed so they are not asked for forever.
            let (materials, textures) = job.join().unwrap_or_else(|_| {
                (
                    requested_dyes
                        .into_iter()
                        .map(|dye| (dye, stopped()))
                        .collect(),
                    requested_tags
                        .into_iter()
                        .map(|tag| (tag, stopped()))
                        .collect(),
                )
            });
            self.materials.extend(materials);
            self.textures.extend(textures);
        }
        self.materials.retain(|dye, _| {
            #[cfg(feature = "d2-model-importer")]
            if self.source_key.is_some() && crate::shader::source_channel(*dye).is_some() {
                return true;
            }
            dyes.contains(dye)
        });
        let bound = self
            .materials
            .values()
            .flatten()
            .flat_map(|material| [material.detail, material.normal])
            .flatten()
            .collect::<BTreeSet<_>>();
        self.textures
            .retain(|tag, _| tags.contains(tag) || bound.contains(tag));
        let textures = &self.textures;
        self.thumbnails.retain(|tag, _| textures.contains_key(tag));
        let missing_dyes = dyes
            .iter()
            .copied()
            .filter(|dye| !self.materials.contains_key(dye))
            .filter(|_dye| {
                #[cfg(feature = "d2-model-importer")]
                if self.source_key.is_some() && crate::shader::source_channel(*_dye).is_some() {
                    return false;
                }
                true
            })
            .collect::<Vec<_>>();
        let missing_tags = tags
            .iter()
            .copied()
            .filter(|tag| !self.textures.contains_key(tag))
            .collect::<Vec<_>>();
        if let Some(((pending_dyes, pending_tags), _)) = &self.job {
            // A read for others finishes first, then the next update starts these.
            if missing_dyes.iter().any(|dye| !pending_dyes.contains(dye))
                || missing_tags.iter().any(|tag| !pending_tags.contains(tag))
            {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return;
        }
        if (missing_dyes.is_empty() && missing_tags.is_empty()) || packages.as_os_str().is_empty() {
            return;
        }
        let packages = packages.to_owned();
        let ctx = ctx.clone();
        let (dyes, tags) = (missing_dyes.clone(), missing_tags.clone());
        self.job = Some((
            (missing_dyes, missing_tags),
            thread::spawn(move || {
                let loaded = read(&packages, &dyes, &tags);
                ctx.request_repaint();
                loaded
            }),
        ));
    }

    pub(super) fn material(&self, dye: u16) -> Option<&Result<Material, String>> {
        self.materials.get(&dye)
    }

    /// A texture's pixels once read, or none while it loads or when it cannot be read.
    pub(super) fn texture(&self, tag: u32) -> Option<&IconTexture> {
        self.textures.get(&tag)?.as_ref().ok()
    }

    /// Whether a texture has been read, successfully or not.
    pub(super) fn texture_read(&self, tag: u32) -> bool {
        self.textures.contains_key(&tag)
    }

    /// A texture shown small on the page, once read. Alpha holds smoothness rather than coverage,
    /// so the thumbnail shows the color alone.
    pub(super) fn thumbnail(
        &mut self,
        ctx: &egui::Context,
        tag: u32,
    ) -> Option<egui::TextureHandle> {
        if let Some(handle) = self.thumbnails.get(&tag) {
            return Some(handle.clone());
        }
        let texture = self.textures.get(&tag)?.as_ref().ok()?;
        let rgb = texture
            .rgba
            .chunks_exact(4)
            .flat_map(|texel| [texel[0], texel[1], texel[2]])
            .collect::<Vec<_>>();
        if rgb.len() != texture.width * texture.height * 3 {
            return None;
        }
        let image = egui::ColorImage::from_rgb([texture.width, texture.height], &rgb);
        let handle = ctx.load_texture(
            format!("parhelion-dye-texture-{tag:08X}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.thumbnails.insert(tag, handle.clone());
        Some(handle)
    }

    /// What one surface of `dye` draws from with `edit` and `textures` written over it, or none
    /// until the dye, the textures it binds and the iridescence ramps have loaded.
    pub(super) fn drawing<'a>(
        &self,
        dye: u16,
        (edit, textures): (Option<DyeEdit>, Option<DyeTextureEdit>),
        surface: DyeSurface,
        iridescence: &'a Iridescence,
    ) -> Option<Drawing<'a>> {
        let Some(Ok(material)) = self.material(dye) else {
            return None;
        };
        let mut vectors = material.vectors;
        if let Some(edit) = edit {
            write_vectors(&mut vectors, &edit.writes());
        }
        let read = dye_surfaces(&vectors)[surface.index()];
        let ramp = match read.iridescence {
            id if id < 0.0 => None,
            id => match iridescence.row(id as i16) {
                Some(row) => Some(row),
                None if iridescence.rows().is_empty() => return None,
                None => None,
            },
        };
        let detail = textures
            .and_then(|textures| textures.detail)
            .or(material.detail);
        let normal = textures
            .and_then(|textures| textures.normal)
            .or(material.normal);
        // A swapped texture draws once it has been read.
        if [detail, normal]
            .into_iter()
            .flatten()
            .any(|tag| !self.texture_read(tag))
        {
            return None;
        }
        Some(Drawing {
            read,
            ramp,
            detail,
            normal,
        })
    }

    /// A surface's ball on the page, `pixels` square, drawn again only when what it shows
    /// changes. None until everything it shows has loaded.
    pub(super) fn swatch(
        &mut self,
        ctx: &egui::Context,
        (dye, edit, textures): (u16, Option<DyeEdit>, Option<DyeTextureEdit>),
        (channel, surface): (DyeChannel, DyeSurface),
        (iridescence, pixels): (&Iridescence, u32),
    ) -> Option<egui::TextureHandle> {
        let drawing = self.drawing(dye, (edit, textures), surface, iridescence)?;
        let mut key = DefaultHasher::new();
        (dye, edit, textures, drawing.ramp.is_some(), pixels).hash(&mut key);
        let key = key.finish();
        let slot = channel.index() * 2 + surface.index();
        if let Some((drawn, handle)) = &self.swatches[slot]
            && *drawn == key
        {
            return Some(handle.clone());
        }
        let image = crate::shader_icon::swatch(&drawing.surface(self), pixels);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );
        let handle = match self.swatches[slot].take() {
            Some((_, mut handle)) => {
                handle.set(image, egui::TextureOptions::LINEAR);
                handle
            }
            None => ctx.load_texture(
                format!("parhelion-dye-swatch-{slot}"),
                image,
                egui::TextureOptions::LINEAR,
            ),
        };
        self.swatches[slot] = Some((key, handle.clone()));
        Some(handle)
    }
}

#[cfg(feature = "d2-model-importer")]
fn read_source(recipe: &WeaponRecipe) -> SourceRead {
    let sources = crate::shader::source_materials(recipe)?;
    let mut materials = Materials::new();
    let mut textures = Textures::new();
    for (&channel, source) in &sources {
        let material = sundial::package_authoring::decode_source_material(source).map(|m| {
            for (tag, texture) in [(m.detail_tag, m.detail), (m.normal_tag, m.normal)] {
                if let (Some(tag), Some(t)) = (tag, texture) {
                    textures.insert(
                        tag,
                        Ok(IconTexture {
                            width: t.width,
                            height: t.height,
                            rgba: t.rgba,
                        }),
                    );
                }
            }
            Material {
                vectors: m.vectors,
                detail: m.detail_tag,
                normal: m.normal_tag,
                detail_tiling: m.detail_transform,
                normal_tiling: m.normal_transform,
            }
        });
        materials.insert(crate::shader::SOURCE_DYE + channel as u16, material);
    }
    let graph = recipe
        .overrides
        .imported_graph
        .as_ref()
        .and_then(|g| std::fs::read(g.directory.join("asset-graph.json")).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let identity = SourceIdentity {
        name: graph
            .as_ref()
            .and_then(|g| g["source_name"].as_str().map(str::to_owned)),
        hash: graph
            .as_ref()
            .and_then(|g| g["source_item"].as_u64())
            .and_then(|hash| u32::try_from(hash).ok()),
        icon: crate::shader::source_icon(recipe).ok(),
    };
    Ok((sources, materials, textures, identity))
}

/// What one surface draws from: its material read after the page's edit, its iridescence ramp,
/// and its textures' tags.
pub(super) struct Drawing<'a> {
    read: DyeSurfaceMaterial,
    pub(super) ramp: Option<&'a IridescenceRow>,
    detail: Option<u32>,
    normal: Option<u32>,
}

impl Drawing<'_> {
    /// The surface as the icon renderer takes it, with its textures from `materials`.
    pub(super) fn surface<'a>(&'a self, materials: &'a DyeMaterials) -> IconSurface<'a> {
        IconSurface {
            paint: finish(&self.read.paint),
            worn: finish(&self.read.worn),
            wear: self.read.wear,
            iridescence: self.ramp.map(|row| RampIridescence {
                colors: &row.colors,
                highlight: row.id.rem_euclid(2) == 1,
            }),
            detail: self.detail.and_then(|tag| materials.texture(tag)),
            normal: self.normal.and_then(|tag| materials.texture(tag)),
        }
    }
}

fn finish(finish: &DyeFinish) -> Finish {
    Finish {
        albedo: finish.albedo,
        params: finish.params,
        smoothness: finish.smoothness,
    }
}

/// What a read that stopped before finishing gives each of its requests.
fn stopped<T>() -> Result<T, String> {
    Err("Dye material loading stopped".to_owned())
}

/// Reads dye materials, with each dye's own textures, and any other textures by tag.
fn read(packages: &Path, dyes: &[u16], tags: &[u32]) -> (Materials, Textures) {
    let pixels = |texture: DyeTexture| IconTexture {
        width: texture.width,
        height: texture.height,
        rgba: texture.rgba,
    };
    let mut textures = Textures::new();
    let materials = match load_dye_materials(packages, dyes) {
        Ok(loaded) => loaded
            .into_iter()
            .map(|(dye, material)| {
                let material = material.map(|material| {
                    for (tag, texture) in [
                        (material.detail_tag, material.detail),
                        (material.normal_tag, material.normal),
                    ] {
                        if let (Some(tag), Some(texture)) = (tag, texture) {
                            textures.insert(tag, Ok(pixels(texture)));
                        }
                    }
                    Material {
                        vectors: material.vectors,
                        detail: material.detail_tag,
                        normal: material.normal_tag,
                        detail_tiling: material.detail_transform,
                        normal_tiling: material.normal_transform,
                    }
                });
                (dye, material)
            })
            .collect(),
        Err(error) => dyes.iter().map(|dye| (*dye, Err(error.clone()))).collect(),
    };
    let others = tags
        .iter()
        .copied()
        .filter(|tag| !textures.contains_key(tag))
        .collect::<Vec<_>>();
    if !others.is_empty() {
        match load_dye_textures(packages, &others) {
            Ok(loaded) => textures.extend(
                loaded
                    .into_iter()
                    .map(|(tag, texture)| (tag, texture.map(pixels))),
            ),
            Err(error) => {
                textures.extend(others.into_iter().map(|tag| (tag, Err(error.clone()))));
            }
        }
    }
    (materials, textures)
}

/// A dye's textures and tiling as the page shows them: its own, with the view's edit over them.
#[derive(Clone, Copy)]
pub(super) struct TextureValues {
    pub(super) detail: Option<u32>,
    pub(super) normal: Option<u32>,
    pub(super) detail_tiling: [f32; 4],
    pub(super) normal_tiling: [f32; 4],
}

impl TextureValues {
    pub(super) fn read(material: &Material, edit: Option<DyeTextureEdit>) -> Self {
        let tiling = |own: Option<[DyeValue; 4]>, stock: [f32; 4]| {
            own.map_or(stock, |tiling| tiling.map(DyeValue::get))
        };
        Self {
            detail: edit.and_then(|edit| edit.detail).or(material.detail),
            normal: edit.and_then(|edit| edit.normal).or(material.normal),
            detail_tiling: tiling(
                edit.and_then(|edit| edit.detail_tiling),
                material.detail_tiling,
            ),
            normal_tiling: tiling(
                edit.and_then(|edit| edit.normal_tiling),
                material.normal_tiling,
            ),
        }
    }
}

/// A surface's values as the page shows them: its dye's, with the page's edits written over them.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Values {
    pub(super) color: [u8; 3],
    pub(super) iridescence: i16,
    pub(super) metalness: f32,
    pub(super) smoothness: [f32; 2],
    pub(super) detail: f32,
    pub(super) bumps: f32,
    pub(super) detail_smoothness: f32,
    pub(super) glow: [u8; 3],
    pub(super) worn_color: [u8; 3],
    pub(super) worn_metalness: f32,
    pub(super) worn_smoothness: [f32; 2],
    pub(super) wear: [f32; 2],
}

impl Values {
    /// One surface's values after `writes`, read the way the build's dye is read back.
    pub(super) fn read(
        vectors: &[[f32; 4]; 27],
        writes: &[(usize, usize, f32)],
        surface: DyeSurface,
    ) -> Self {
        let mut vectors = *vectors;
        write_vectors(&mut vectors, writes);
        let read = dye_surfaces(&vectors)[surface.index()];
        // A range is its least value and how far the most lies past it, either way.
        let range = |remap: [f32; 4]| {
            let end = remap[2] + remap[3];
            [remap[2].min(end), remap[2].max(end)]
        };
        Self {
            color: read.paint.albedo.map(linear_to_srgb),
            iridescence: if read.iridescence < 0.0 {
                NO_IRIDESCENCE
            } else {
                read.iridescence as i16
            },
            metalness: read.paint.params[3],
            smoothness: range(read.paint.smoothness),
            detail: read.paint.params[0],
            bumps: read.paint.params[1],
            detail_smoothness: read.paint.params[2],
            glow: read.emissive.map(linear_to_srgb),
            worn_color: read.worn.albedo.map(linear_to_srgb),
            worn_metalness: read.worn.params[3],
            worn_smoothness: range(read.worn.smoothness),
            wear: [read.wear[0], read.wear[1]],
        }
    }
}
