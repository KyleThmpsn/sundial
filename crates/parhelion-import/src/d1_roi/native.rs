//! Native resource assembly for source armor.
use super::*;
use crate::tiger::reader::Reader;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) mod animation;
mod component;
mod dye;
mod geometry;
mod icon;
mod material;
mod perk;
mod plate;
mod recipe;
mod resource;
mod rig;
mod weapon;

#[derive(Default)]
pub struct NativeOptions {
    /// Retained for existing callers. Texture conversion is built in and ignores this path.
    pub xg_library: Option<PathBuf>,
    /// A saved recipe's namespace that a reimport keeps, so the authored item retains its
    /// identity and edits whichever naming scheme made it. Unset for a new import.
    pub namespace: Option<String>,
}

struct Assembly<'a> {
    source: &'a Source,
    native: Reader,
    options: &'a NativeOptions,
    folder: PathBuf,
    nodes: Vec<Value>,
    symbols: BTreeSet<String>,
    templates: BTreeMap<(u8, u8), u32>,
    globals: super::expression::Globals,
    layouts: super::layout::Layouts,
    glow: Vec<super::shader::glow::Profile>,
    evidence: Vec<Value>,
    /// Explicit translation choices the graph reports with its limitations.
    approximations: BTreeSet<String>,
    /// The current body's channel bank names in ordinal order, which its materials' object
    /// inputs index.
    object_channels: Vec<u32>,
    palette: u32,
    /// Source equipment index to native index. Present only when runtime palettes differ.
    bone_map: Option<Vec<u16>>,
    weapon: bool,
    weapon_runtime: Option<u32>,
}

impl<'a> Assembly<'a> {
    fn new(
        source: &'a Source,
        packages: &Path,
        folder: &Path,
        options: &'a NativeOptions,
        weapon: Option<(&Value, u32)>,
    ) -> Result<Self> {
        let mut native = Reader::new(packages, &folder.join("native-source"), false)?;
        let globals = super::expression::Globals::read(source, &mut native)?;
        let layouts = super::layout::Layouts::read(source, &mut native)?;
        let glow = super::shader::glow::read(source, options.xg_library.as_deref())?;
        let rig = if let Some((runtime, donor)) = weapon {
            rig::weapon(source, runtime, donor, &mut native)?
        } else {
            rig::validate(source, &mut native)?
        };
        let palette = u32::try_from(rig["bones"].as_u64().context("Validated palette size")?)?;
        let bone_map = (rig["remapped"] == true)
            .then(|| serde_json::from_value(rig["bone_map"].clone()))
            .transpose()?;
        fs::create_dir_all(folder)?;
        Ok(Self {
            source,
            native,
            options,
            folder: folder.to_owned(),
            nodes: Vec::new(),
            symbols: BTreeSet::new(),
            templates: BTreeMap::new(),
            globals,
            layouts,
            glow,
            evidence: vec![rig],
            approximations: BTreeSet::new(),
            object_channels: Vec::new(),
            palette,
            bone_map,
            weapon: weapon.is_some(),
            weapon_runtime: weapon
                .map(|(runtime, _)| {
                    u32::from_str_radix(
                        runtime["runtime_entity"]
                            .as_str()
                            .context("Source weapon runtime")?,
                        16,
                    )
                    .map_err(anyhow::Error::from)
                })
                .transpose()?,
        })
    }

    fn template(&mut self, kind: u8, subtype: u8) -> Result<u32> {
        if let Some(tag) = self.templates.get(&(kind, subtype)) {
            return Ok(*tag);
        }
        let mut tags = Vec::new();
        for (&package, entries) in &self.native.manager.lookup.tag32_entries_by_pkg {
            for (index, entry) in entries.iter().enumerate() {
                if entry.file_type == kind && entry.file_subtype == subtype {
                    tags.push(TagHash::new(package, u16::try_from(index)?).0);
                }
            }
        }
        tags.sort_unstable();
        let tag = *tags
            .first()
            .with_context(|| format!("Native resource type {kind}/{subtype} has no template"))?;
        self.templates.insert((kind, subtype), tag);
        Ok(tag)
    }

    fn class_template(&self, class: u32) -> Result<u32> {
        self.native
            .classes(class)
            .into_iter()
            .min()
            .with_context(|| format!("Native class {class:08X} has no allocation template"))
    }

    fn node(
        &mut self,
        symbol: &str,
        template: u32,
        payload: &[u8],
        reference: Option<&str>,
        patches: &[(usize, String)],
    ) -> Result<String> {
        ensure!(
            self.symbols.insert(symbol.to_owned()),
            "Duplicate native armor asset {symbol}"
        );
        for (offset, _) in patches {
            ensure!(
                payload.get(*offset..offset + 4) == Some(&u32::MAX.to_le_bytes()[..]),
                "Native armor relocation has no placeholder at {offset:X}"
            );
        }
        let file = format!("{symbol}.bin");
        fs::write(self.folder.join(&file), payload)?;
        self.nodes.push(json!({"symbol":symbol,"file":file,"template":template,"reference":reference,
            "patches":patches.iter().map(|(offset,symbol)|json!({"offset":offset,"symbol":symbol})).collect::<Vec<_>>()}));
        Ok(symbol.to_owned())
    }
}

fn put(data: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    data.get_mut(
        at..at
            .checked_add(bytes.len())
            .context("Native write overflow")?,
    )
    .context("Native write exceeds payload")?
    .copy_from_slice(bytes);
    Ok(())
}

fn array(
    data: &mut Vec<u8>,
    field: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<usize> {
    ensure!(
        stride > 0 && rows.len().is_multiple_of(stride),
        "Native armor array extent differs"
    );
    if rows.is_empty() {
        put(data, field, &[0; 16])?;
        return Ok(0);
    }
    let header = (data.len() + 4).next_multiple_of(16);
    data.resize(header, 0);
    put(data, header - 4, &0x80809FBDu32.to_le_bytes())?;
    let count = (rows.len() / stride) as u64;
    data.extend(count.to_le_bytes());
    data.extend(u64::from(class).to_le_bytes());
    data.extend(rows);
    put(data, field, &count.to_le_bytes())?;
    put(
        data,
        field + 8,
        &(header as i64 - (field + 8) as i64).to_le_bytes(),
    )?;
    Ok(header + 16)
}

fn size(data: &mut [u8]) -> Result<()> {
    let length = data.len() as u64;
    put(data, 0, &length.to_le_bytes())
}
