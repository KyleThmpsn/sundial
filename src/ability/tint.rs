//! The color constants of an ability's particle materials. Many effects draw a grayscale ramp and
//! take their color from a constant of the material's pixel stage, the shader's `cb0`. Which
//! constant is a color is read from the shader: `shaders.json` lists, for every pixel shader the
//! stock abilities' particles use, each `cb0` constant whose x, y and z reach the output's red,
//! green and blue channel for channel. `examples/tint_shader_census.rs` and
//! `examples/tint_constants.py` produce it from the shaders' disassembly. A constant the table
//! does not list is left alone, since other constants of the same materials hold parameters.
use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::palette::{
    MATERIAL_CLASS, ParticleSite, SYSTEM_MATERIAL, ability_graphs, particle_sites,
};
use crate::package_payload::{native_array_at, u32_at};
use crate::package_runtime::reader::PackageManager;

/// A material's pixel stage: its shader, its inline constants, and the constant buffer it names
/// in their place.
const PIXEL_SHADER: usize = 0x2C8;
const INLINE_CONSTANTS: usize = PIXEL_SHADER + 0x50;
const EXTERNAL_CONSTANTS: usize = PIXEL_SHADER + 0x84;
const CONSTANT_CLASS: u32 = 0x8080_0090;
const CONSTANT_SIZE: usize = 16;
/// The most constants a stage holds: Direct3D 11's limit of 4096 four-float rows per constant
/// buffer. Stock particle materials hold up to 129.
const CONSTANT_LIMIT: usize = 4096;
/// An external buffer's header and data name each other, as a texture's do.
const BUFFER_HEADER: (u8, u8) = (32, 7);
const BUFFER_DATA: (u8, u8) = (40, 7);
/// A constant reads as a color when its brightest channel shows and the others differ from it.
const LEAST_CHROMA: f32 = 0.1;
const LEAST_VALUE: f32 = 0.02;

fn table() -> &'static BTreeMap<u32, Vec<usize>> {
    static TABLE: OnceLock<BTreeMap<u32, Vec<usize>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: BTreeMap<String, Vec<usize>> =
            serde_json::from_str(include_str!("tint/shaders.json"))
                .expect("the tint shader table is valid JSON");
        raw.into_iter()
            .filter_map(|(shader, constants)| {
                Some((u32::from_str_radix(&shader, 16).ok()?, constants))
            })
            .collect()
    })
}

/// Where a material keeps its pixel stage constants.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConstantStore {
    /// In the material's own payload.
    Inline,
    /// In a buffer the material names: its header tag and the data tag the header names.
    External { header: u32, data: u32 },
}

/// One color constant of a material: its `cb0` index, where its x, y and z sit in the material
/// payload or the buffer data, and its stock color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorConstant {
    pub index: usize,
    pub offset: usize,
    pub rgb: [f32; 3],
}

/// Whether a stock color is one a tint lists: bright enough to see and not a gray.
#[must_use]
pub fn is_tint(rgb: [f32; 3]) -> bool {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    rgb.iter()
        .all(|channel| channel.is_finite() && *channel >= 0.0)
        && max > LEAST_VALUE
        && (max - min) / max > LEAST_CHROMA
}

/// The material's constant store and its color constants. A material whose shader the table
/// does not list has none, and its constants are not read.
pub fn color_constants(
    manager: &PackageManager,
    material: u32,
    payload: &[u8],
) -> Result<(ConstantStore, Vec<ColorConstant>), String> {
    let context = |error: String| format!("Effect material 0x{material:08X}: {error}");
    let shader = u32_at(payload, PIXEL_SHADER).map_err(context)?;
    let Some(colors) = table().get(&shader) else {
        return Ok((ConstantStore::Inline, Vec::new()));
    };
    let external = u32_at(payload, EXTERNAL_CONSTANTS).map_err(context)?;
    let (store, data, rows) = if matches!(external, 0 | u32::MAX | 0x811C_9DC5) {
        let (count, _, rows, class) =
            native_array_at(payload, INLINE_CONSTANTS).map_err(context)?;
        if count > 0 && (class != CONSTANT_CLASS || count > CONSTANT_LIMIT) {
            return Err(context(format!("its constants are class 0x{class:08X}")));
        }
        (
            ConstantStore::Inline,
            None,
            rows..rows + count * CONSTANT_SIZE,
        )
    } else {
        let header = manager
            .get_entry(external)
            .ok_or_else(|| context(format!("its constant buffer 0x{external:08X} is missing")))?;
        let data_tag = header.reference;
        let data_entry = manager
            .get_entry(data_tag)
            .ok_or_else(|| context(format!("its buffer data 0x{data_tag:08X} is missing")))?;
        if (header.file_type, header.file_subtype) != BUFFER_HEADER
            || (data_entry.file_type, data_entry.file_subtype) != BUFFER_DATA
            || data_entry.reference != external
        {
            return Err(context(format!(
                "constant buffer 0x{external:08X} is not a header and data pair"
            )));
        }
        let data = manager.read_tag(data_tag)?;
        if data.len() % CONSTANT_SIZE != 0 || data.len() > CONSTANT_LIMIT * CONSTANT_SIZE {
            return Err(context("its constant buffer has an odd length".into()));
        }
        let length = data.len();
        (
            ConstantStore::External {
                header: external,
                data: data_tag,
            },
            Some(data),
            0..length,
        )
    };
    let bytes = data.as_deref().unwrap_or(payload);
    let mut found = Vec::new();
    for &index in colors {
        let offset = rows.start + index * CONSTANT_SIZE;
        if offset + 12 > rows.end {
            continue;
        }
        let mut rgb = [0.0; 3];
        for (channel, value) in rgb.iter_mut().enumerate() {
            *value = f32::from_bits(u32_at(bytes, offset + channel * 4)?);
        }
        found.push(ColorConstant { index, offset, rgb });
    }
    Ok((store, found))
}

/// One place a tint reaches an ability: the graph and particle site, its render material, how
/// the material keeps its constants, and the constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TintUse {
    pub graph: u32,
    pub site: ParticleSite,
    pub material: u32,
    pub store: ConstantStore,
    pub constant: ColorConstant,
}

/// A stock color an ability's effects draw with, and every place they reach it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tint {
    pub rgb: [f32; 3],
    pub uses: Vec<TintUse>,
}

/// The tints an ability's effects draw with, across the graphs `ability_graphs` walks, each
/// stock color once, in the order their first use is found.
pub fn ability_tints(
    manager: &PackageManager,
    entity: u32,
    depth: usize,
) -> Result<Vec<Tint>, String> {
    let mut found = Vec::<Tint>::new();
    let mut materials = BTreeMap::<u32, Option<(ConstantStore, Vec<ColorConstant>)>>::new();
    for (graph, payload) in ability_graphs(manager, entity, depth)? {
        for site in particle_sites(manager, &payload)? {
            let system = manager.read_tag(site.system)?;
            let material = u32_at(&system, SYSTEM_MATERIAL)?;
            // A material whose constants do not read is left stock, and the ability's other
            // materials still list their tints.
            let Some((store, constants)) = materials.entry(material).or_insert_with(|| {
                manager
                    .get_entry(material)
                    .filter(|entry| entry.file_type == 8 && entry.reference == MATERIAL_CLASS)
                    .and_then(|_| {
                        let bytes = manager.read_tag(material).ok()?;
                        color_constants(manager, material, &bytes).ok()
                    })
            }) else {
                continue;
            };
            for constant in constants.iter().filter(|constant| is_tint(constant.rgb)) {
                let tint_use = TintUse {
                    graph,
                    site,
                    material,
                    store: *store,
                    constant: *constant,
                };
                let same = |tint: &&mut Tint| {
                    tint.rgb
                        .iter()
                        .zip(constant.rgb)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                };
                match found.iter_mut().find(same) {
                    Some(tint) => tint.uses.push(tint_use),
                    None => found.push(Tint {
                        rgb: constant.rgb,
                        uses: vec![tint_use],
                    }),
                }
            }
        }
    }
    Ok(found)
}
