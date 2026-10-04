//! Discover rendering carriers by their format contracts, never by asset identity.
use super::*;
use crate::d2_mot::reader::Reader;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Role {
    Surface,
    Decal,
    AdditiveDecal,
    AlphaDecal,
    Transparent,
    Emission,
    Shadow,
    Depth,
    OpticStencil,
    Reticle,
}

impl Role {
    fn accepts_layout(self, layout: i16) -> bool {
        layout == 139 || (layout == 137 && matches!(self, Self::Reticle | Self::OpticStencil))
    }
    const ALL: [Self; 10] = [
        Self::Surface,
        Self::Decal,
        Self::AdditiveDecal,
        Self::AlphaDecal,
        Self::Transparent,
        Self::Emission,
        Self::Shadow,
        Self::Depth,
        Self::OpticStencil,
        Self::Reticle,
    ];

    fn stage(self) -> usize {
        match self {
            Self::Surface => 0,
            Self::Decal | Self::AdditiveDecal | Self::AlphaDecal => 1,
            Self::Transparent => 7,
            Self::Emission => 9,
            Self::Shadow => 3,
            Self::Depth => 12,
            Self::OpticStencil => 14,
            Self::Reticle => 16,
        }
    }

    /// These are engine render states and scope bits, not material tag IDs.
    fn accepts(self, p: &Payload) -> Result<bool> {
        let (mode, scopes, state) = (p.u32(8)?, p.u32(24)?, p.u32(32)?);
        let pixel = p.u32(0x2C8)?;
        let valid = match self {
            Self::Surface => mode == 1 && scopes == 0x06000083 && state == 0,
            Self::Decal => mode == 1 && scopes == 0x06000483 && state == 0x9A,
            Self::AdditiveDecal => mode == 1 && state == 0x9D,
            Self::AlphaDecal => mode == 1 && state == 0xB9,
            Self::Transparent => mode == 1 && scopes & !0x06004000 == 0x2083 && state == 0x88,
            Self::Emission => mode == 1 && scopes == 0x2083 && state == 0x88,
            Self::Shadow => mode == 2 && scopes == 0x8083 && state == 0 && pixel == u32::MAX,
            Self::Depth => mode == 2 && scopes == 0x83 && state == 0 && pixel == u32::MAX,
            Self::OpticStencil => mode == 1 && scopes == 0x83 && state == 0,
            Self::Reticle => mode == 1 && scopes == 0x04002083 && state == 0x88,
        };
        if !valid || p.u32(0x48)? == u32::MAX {
            return Ok(false);
        }
        if !matches!(self, Self::Shadow | Self::Depth) && pixel == u32::MAX {
            return Ok(false);
        }
        // Fields the adapter retains must have the audited engine meaning.
        // Matching the blend selector alone would admit unrelated material families.
        let envelope = match self {
            Self::Surface => Some([2, 0, 0x400000, 0x86000083, 0x7F7F80]),
            Self::Decal => Some([0, 0x1000, 0x400000, 0x86000483, 0x7F7F80]),
            Self::Emission => Some([0, 0, 0, 0x02006083, 0x7F7F00]),
            Self::Shadow | Self::Depth => Some([0, 0, 0, 0x82008083, 0x10180]),
            Self::OpticStencil => Some([0, 0, 0, 0x02000083, 0x7F7F00]),
            Self::Reticle => Some([4, 0, 0x10000200, 0x06002083, 0x7F7F00]),
            _ => None,
        };
        if let Some(expected) = envelope {
            for (at, value) in [12, 16, 20, 28, 36].into_iter().zip(expected) {
                if p.u32(at)? != value {
                    return Ok(false);
                }
            }
            if p.u32(40)? != u32::MAX || p.0[44..72].iter().any(|&b| b != 0) {
                return Ok(false);
            }
        }
        // The converter replaces vertex and pixel programs. It does not adapt
        // tessellation or geometry programs hidden in another shader stage.
        for base in [0xE8, 0x188, 0x228, 0x368] {
            if p.u32(base)? != u32::MAX {
                return Ok(false);
            }
        }
        for base in [0x48, 0x2C8] {
            for (offset, stride) in [(8, 8), (0x20, 1), (0x30, 16), (0x40, 16), (0x50, 16)] {
                p.array(base + offset, stride, None)?;
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Carrier {
    pub material: u32,
    model: u32,
    mesh: usize,
    stage: usize,
    layout: i16,
    pub record: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Catalog {
    schema: u32,
    stamp: String,
    carriers: BTreeMap<Role, Carrier>,
    pub cube: Value,
    pub samplers: Vec<Value>,
}

impl Catalog {
    const SCHEMA: u32 = 2;

    pub fn read(root: &Path) -> Result<Self> {
        let catalog: Self = serde_json::from_value(load(&root.join("contracts.json"))?)?;
        ensure!(
            catalog.schema == Self::SCHEMA,
            "Native rendering cache version differs"
        );
        Ok(catalog)
    }

    pub fn carrier(&self, role: Role) -> Result<&Carrier> {
        self.carriers.get(&role).with_context(|| {
            format!("No compatible native {role:?} rendering carrier in the configured packages")
        })
    }

    pub fn material(&self, root: &Path, role: Role) -> Result<Payload> {
        let tag = self.carrier(role)?.material;
        let payload = Payload(fs::read(root.join(format!("materials/raw/{tag:08X}.bin")))?);
        ensure!(
            role.accepts(&payload)?,
            "Native {role:?} material no longer matches its contract"
        );
        Ok(payload)
    }

    fn validate(&self, reader: &Reader) -> Result<()> {
        for (&role, carrier) in &self.carriers {
            let model = Payload(reader.manager.read_tag(tiger_pkg::TagHash(carrier.model))?);
            let material = Payload(
                reader
                    .manager
                    .read_tag(tiger_pkg::TagHash(carrier.material))?,
            );
            ensure!(role.accepts(&material)?, "Cached {role:?} material changed");
            ensure!(
                model
                    .array(16, 136, Some(0x80807378))?
                    .contains(&carrier.mesh),
                "Cached native mesh changed"
            );
            let current = draw(
                &model,
                carrier.model,
                carrier.mesh,
                carrier.stage,
                carrier.material,
            )?;
            ensure!(
                carrier.stage == role.stage()
                    && role.accepts_layout(carrier.layout)
                    && current.layout == carrier.layout
                    && current.record == carrier.record,
                "Cached native draw changed"
            );
        }
        Ok(())
    }
}

fn draw(model: &Payload, tag: u32, mesh: usize, stage: usize, material: u32) -> Result<Carrier> {
    ensure!(
        stage < 23 && matches!(model.i16(mesh + 88 + stage * 2)?, 137 | 139),
        "Native draw layout differs"
    );
    let rows = model.array(mesh + 24, 32, Some(0x8080737E))?;
    let range =
        model.u16(mesh + 40 + stage * 2)? as usize..model.u16(mesh + 42 + stage * 2)? as usize;
    let at = rows
        .get(range)
        .context("Native draw range")?
        .iter()
        .copied()
        .find(|&at| model.u32(at).ok() == Some(material))
        .context("Native material draw is missing")?;
    Ok(Carrier {
        material,
        model: tag,
        mesh,
        stage,
        layout: model.i16(mesh + 88 + stage * 2)?,
        record: model.0[at..at + 32].to_vec(),
    })
}

fn select_model(
    tag: u32,
    model: &Payload,
    carriers: &mut BTreeMap<Role, Carrier>,
    mut read_material: impl FnMut(u32) -> Option<Payload>,
) -> Result<()> {
    for mesh in model.array(16, 136, Some(0x80807378))? {
        let Ok(rows) = model.array(mesh + 24, 32, Some(0x8080737E)) else {
            continue;
        };
        for role in Role::ALL {
            if carriers.contains_key(&role) {
                continue;
            }
            let stage = role.stage();
            if !model
                .i16(mesh + 88 + stage * 2)
                .is_ok_and(|layout| role.accepts_layout(layout))
            {
                continue;
            }
            let start = model.u16(mesh + 40 + stage * 2)? as usize;
            let end = model.u16(mesh + 42 + stage * 2)? as usize;
            let Some(parts) = rows.get(start..end) else {
                continue;
            };
            for &at in parts {
                let material = model.u32(at)?;
                let payload = read_material(material);

                if payload
                    .as_ref()
                    .is_some_and(|p| role.accepts(p).unwrap_or(false))
                {
                    carriers.insert(role, draw(model, tag, mesh, stage, material)?);
                    break;
                }
            }
        }
    }
    Ok(())
}

fn scan(reader: &Reader, stamp: String, progress: &mut dyn FnMut(String)) -> Result<Catalog> {
    let mut carriers = BTreeMap::new();
    let mut materials = BTreeMap::<u32, Option<Payload>>::new();
    let mut models = reader.classes(0x808073A5);
    models.sort_unstable();
    for (index, tag) in models.iter().copied().enumerate() {
        if index % 128 == 0 {
            progress(format!(
                "Finding compatible native rendering passes: {} of {} models…",
                index + 1,
                models.len()
            ));
        }
        let Ok(bytes) = reader.manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        let model = Payload(bytes);
        // Unrelated assets can use layouts this converter does not understand.
        // They are not candidates and must not prevent finding a later match.
        let _ = select_model(tag, &model, &mut carriers, |material| {
            materials
                .entry(material)
                .or_insert_with(|| {
                    let entry = reader.manager.get_entry(tiger_pkg::TagHash(material))?;
                    if entry.reference != 0x808071E8 {
                        return None;
                    }
                    reader
                        .manager
                        .read_tag(tiger_pkg::TagHash(material))
                        .ok()
                        .map(Payload)
                })
                .clone()
        });
        if carriers.len() == Role::ALL.len() {
            break;
        }
    }
    progress("Finding native texture and sampler formats…".into());
    let (cube, samplers) = resources(reader)?;
    Ok(Catalog {
        schema: Catalog::SCHEMA,
        stamp,
        carriers,
        cube,
        samplers,
    })
}

fn resources(reader: &Reader) -> Result<(Value, Vec<Value>)> {
    let mut entries = reader
        .manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .flat_map(|(&pkg, rows)| {
            rows.iter()
                .enumerate()
                .filter(|(_, e)| e.file_type == 34 || (e.file_type == 32 && e.file_subtype == 2))
                .map(move |(i, e)| {
                    (
                        tiger_pkg::TagHash::new(pkg, i as u16).0,
                        e.file_type,
                        e.reference,
                    )
                })
        })
        .collect::<Vec<_>>();
    entries.sort_unstable();
    let mut cube = Value::Null;
    let mut samplers = Vec::new();
    let mut seen = BTreeSet::new();
    for (tag, kind, reference) in entries {
        if kind == 32 && !cube.is_null() {
            continue;
        }
        let Ok(bytes) = reader.manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        let header = Payload(bytes);
        if kind == 32 && header.0.len() == 40 && header.u16(18)? == 1 && header.u16(20)? == 6 {
            if reader
                .manager
                .get_entry(tiger_pkg::TagHash(reference))
                .is_some_and(|e| e.file_type == 40)
            {
                cube = json!({"tag":format!("{tag:08X}"),"buffer":format!("{reference:08X}"),"header":hex::encode(&header.0)});
            }
        } else if kind == 34 && header.0.len() == 8 {
            let Ok(data) = reader.manager.read_tag(tiger_pkg::TagHash(reference)) else {
                continue;
            };
            if data.len() == 52 && seen.insert((header.0.clone(), data.clone())) {
                samplers.push(json!({"tag":format!("{tag:08X}"),"buffer":format!("{reference:08X}"),"header":hex::encode(&header.0),"data":hex::encode(data),"direct_sampler":true}));
            }
        }
    }
    ensure!(
        !samplers.is_empty(),
        "No compatible native sampler format found"
    );
    Ok((cube, samplers))
}

fn reusable_carriers(cache: &Path, current: &Path, reader: &Reader) -> Option<Catalog> {
    let mut candidates = fs::read_dir(cache)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            (path != current
                && path.extension().is_some_and(|ext| ext == "json")
                && path
                    .file_name()?
                    .to_str()?
                    .starts_with(&format!("{}-", Catalog::SCHEMA)))
            .then_some((entry.metadata().ok()?.modified().ok()?, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().find_map(|(_, path)| {
        let bytes = fs::read(path).ok()?;
        let catalog: Catalog = serde_json::from_slice(&bytes).ok()?;
        (catalog.schema == Catalog::SCHEMA && catalog.validate(reader).is_ok()).then_some(catalog)
    })
}

pub(super) fn export(
    reader: &mut Reader,
    packages: &Path,
    root: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    let stamp = crate::d2_mot::service::package_stamp(packages)?;
    let generation = std::env::var_os("LOCALAPPDATA").and_then(|root| {
        crate::cache::Generation::file(
            &PathBuf::from(root).join("Sundial/parhelion/importer/cache/native-contracts"),
            &format!("{}-{stamp}", Catalog::SCHEMA),
        )
    });
    let lock = generation
        .as_ref()
        .map(crate::cache::Generation::writer)
        .transpose()?
        .flatten();
    let generation = generation.filter(|_| lock.is_some());
    let path = generation.as_ref().map(crate::cache::Generation::path);
    let cached = path
        .and_then(|path| fs::read(path).ok())
        .and_then(|b| serde_json::from_slice::<Catalog>(&b).ok())
        .filter(|c| c.schema == Catalog::SCHEMA && c.stamp == stamp && c.validate(reader).is_ok());
    let changed = cached.is_none();
    let catalog = if let Some(catalog) = cached {
        progress("Using cached native rendering compatibility…".into());
        catalog
    } else if let Some(mut catalog) =
        path.and_then(|path| reusable_carriers(path.parent()?, path, reader))
    {
        progress("Reusing validated native rendering passes and refreshing resources…".into());
        (catalog.cube, catalog.samplers) = resources(reader)?;
        catalog.stamp = stamp.clone();
        catalog
    } else {
        scan(reader, stamp.clone(), progress)?
    };
    ensure!(
        crate::d2_mot::service::package_stamp(packages)? == stamp,
        "Native packages changed during discovery"
    );
    if changed && let Some(path) = path {
        let _ = crate::cache::write_json(path, &catalog);
    }
    reader.begin_export(&root.join("materials"))?;
    for carrier in catalog.carriers.values() {
        reader.tag(carrier.material, Some(0x808071E8))?;
    }
    reader.finish()?;
    write_json(
        &root.join("contracts.json"),
        &serde_json::to_value(&catalog)?,
    )?;
    // Only allocation headers are needed here. Source equations supply bytecode.
    let material = catalog.material(root, Role::Surface)?;
    let shader_root = root.join("shaders");
    fs::create_dir_all(&shader_root)?;
    for (name, at) in [("vertex", 0x48), ("pixel", 0x2C8)] {
        let tag = material.u32(at)?;
        let header = reader.tag(tag, None)?;
        let reference = reader.reference(tag)?;
        fs::write(shader_root.join(format!("{name}.header.bin")), &header.0)?;
        write_json(
            &shader_root.join(format!("{name}.meta.json")),
            &json!({"tag":tag,"reference":reference}),
        )?;
    }
    Ok(())
}
