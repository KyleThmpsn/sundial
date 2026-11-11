//! Native resource assembly for source armor.
use super::*;
use crate::tiger::reader::Reader;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) mod animation;
mod component;
pub(crate) mod damage_profile;
mod dye;
mod geometry;
mod icon;
mod material;
pub(crate) mod perk;
mod plate;
mod recipe;
mod resource;
mod rig;
mod runtime_marker;
mod weapon;

pub struct NativeOptions {
    /// Retained for existing callers. Texture conversion is built in and ignores this path.
    pub xg_library: Option<PathBuf>,
    /// A saved recipe's namespace that a reimport keeps, so the authored item retains its
    /// identity and edits whichever naming scheme made it. Unset for a new import.
    pub namespace: Option<String>,
    /// The item's talent grid ornaments import with it as private appearance choices. Each
    /// ornament converts a model of its own.
    pub ornaments: bool,
}

impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            xg_library: None,
            namespace: None,
            ornaments: true,
        }
    }
}

struct Assembly<'a> {
    source: &'a Source,
    native: Reader,
    options: &'a NativeOptions,
    folder: PathBuf,
    nodes: Vec<Value>,
    symbols: BTreeSet<String>,
    /// Complete graph audio, shared by retained animation clips and private talent programs.
    /// Its storage and namespace metadata must remain separate from ordinary appearance nodes.
    audio_assets: Option<Value>,
    /// Private human actions the item's evades play, which the build adds to the shared human
    /// tables from the graph's `human_actions` section.
    human: Option<crate::d1_roi::ability::animation::Human>,
    /// Cues a translated projectile spawn names, which join the graph audio closure.
    projectile_cues: BTreeSet<u32>,
    /// Channels the private bank of a retained projectile declares, empty while the donor's
    /// projectile stays.
    projectile_names: BTreeSet<u32>,
    templates: BTreeMap<(u8, u8), u32>,
    globals: super::expression::Globals,
    layouts: super::layout::Layouts,
    glow: Vec<super::shader::glow::Profile>,
    evidence: Vec<Value>,
    /// Explicit translation choices the graph reports with its limitations.
    approximations: BTreeSet<String>,
    /// The current model component's input names in order, which its materials' object inputs
    /// index.
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
            audio_assets: None,
            human: None,
            projectile_cues: BTreeSet::new(),
            projectile_names: BTreeSet::new(),
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

impl Source {
    /// Lower every distinct talent step of every source weapon and armor piece and report how
    /// many translate, by distinct step, across items and by talent name. `folder` receives the
    /// native reader's working files and any imported attachment groups.
    pub fn talent_census(&self, packages: &Path, folder: &Path) -> Result<Value> {
        fs::create_dir_all(folder)?;
        let mut native = Reader::new(packages, &folder.join("native-source"), false)?;
        perk::census(self, &mut native, folder)
    }
}
