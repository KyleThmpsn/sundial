//! Particle-only model selection keeps the source float streams and the first draw's range.
use crate::{
    d1_roi::{Source, dependency},
    d2_mot::particles::mesh::{Buffer, Geometry, Kind, Reference},
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use tiger_pkg::TagHash;

pub struct Container {
    models: Vec<u32>,
}

impl Container {
    pub fn models(&self) -> &[u32] {
        &self.models
    }

    /// Keep all ordered model references, including repeats. Model and material preparation
    /// must complete separately before this fragment can enter a particle system closure.
    pub fn write(&self, symbol: &str, models: &BTreeMap<u32, String>) -> Result<dependency::Asset> {
        dependency::symbol(symbol)?;
        ensure!(
            self.models
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                == models.keys().copied().collect(),
            "Particle container mappings do not cover exactly its models"
        );
        let mut payload = Payload(vec![0; 32]);
        put(&mut payload.0, 0, &32u64.to_le_bytes())?;
        put(&mut payload.0, 8, &u32::MAX.to_le_bytes())?;
        append(
            &mut payload.0,
            16,
            0x808073A4,
            &u32::MAX.to_le_bytes().repeat(self.models.len()),
            4,
        )?;
        let mut patches = Vec::new();
        for (row, source) in payload
            .array(16, 4, Some(0x808073A4))?
            .into_iter()
            .zip(&self.models)
        {
            let target = &models[source];
            dependency::symbol(target)?;
            ensure!(target != symbol, "Particle container refers to itself");
            patches.push(dependency::Patch {
                offset: row,
                symbol: target.clone(),
                class: 0x808073A5,
            });
        }
        Ok(dependency::Asset {
            symbol: symbol.into(),
            class: 0x80806E2E,
            payload,
            patches,
        })
    }
}

impl Source {
    pub fn particle_container(&self, tag: u32) -> Result<Container> {
        ensure!(
            self.reference(tag)? == 0x80801BFD,
            "Source particle container class differs"
        );
        let source = self.read(tag)?;
        ensure!(
            source.0.len() >= 32
                && source.u64(0)? == source.0.len() as u64
                && source.u32(8)? == u32::MAX
                && source.u32(12)? == 0,
            "Source particle container has another envelope or direct dependency"
        );
        ensure!(
            source.u64(16)? != 0 || source.u64(24)? == 0,
            "Empty source particle container has a pointer"
        );
        let models = source
            .array(16, 4, Some(0x808043A5))?
            .into_iter()
            .map(|at| {
                let tag = source.u32(at)?;
                ensure!(
                    self.reference(tag)? == 0x80801AB5,
                    "Source particle container names another model class"
                );
                Ok(tag)
            })
            .collect::<Result<Vec<_>>>()?;
        self.unchanged()?;
        Ok(Container { models })
    }

    /// Source and native particle mode one both select only the first mesh and first draw,
    /// ignoring its material in favor of the wrapper's material. This canonical model is
    /// for that consumer, not a replacement for the complete equipment model translator.
    /// Its material remains an explicit dependency so loading never retains a source tag.
    pub fn particle_geometry(&self, tag: u32) -> Result<Geometry> {
        ensure!(
            self.reference(tag)? == 0x80801AB5,
            "Source particle model class differs"
        );
        let source = self.read(tag)?;
        ensure!(
            source.0.len() >= 0x50
                && source.u64(0)? == source.0.len() as u64
                && source.u64(8)? == 0
                && source.u64(0x30)? == 32
                && source.u64(0x40)? == 0,
            "Source particle model is not an unskinned float-stream model"
        );
        let meshes = source.array(16, 160, Some(0x80801BBF))?;
        let [mesh] = meshes.as_slice() else {
            anyhow::bail!("Source particle model needs one transform-bearing mesh");
        };
        let mesh = *mesh;
        ensure!(
            [0x34, 0x38, 0x3C]
                .into_iter()
                .all(|at| source.u32(mesh + at).ok() == Some(u32::MAX))
                && source.u32(mesh + 0x44)? == 0
                && source.bytes::<19>(mesh + 0x80)? == [14; 19]
                && source.bytes::<13>(mesh + 0x93)? == [0; 13],
            "Source particle model has another stream, layout or extension"
        );
        let parts = source.array(mesh + 0x48, 36, Some(0x80801AEF))?;
        let part = *parts.first().context("Source particle model has no draw")?;
        ensure!(
            source.u16(part + 4)? == u16::MAX
                && source.u16(part + 6)? == 3
                && source.u32(part + 8)? == 0,
            "Source particle draw needs an unsupported variant, topology or first index"
        );
        let count = usize::try_from(source.u32(part + 12)?)?;
        ensure!(
            count > 0 && count <= usize::from(u16::MAX) && count.is_multiple_of(3),
            "Source particle draw exceeds the original triangle-count contract"
        );
        let vertex = buffer(self, source.u32(mesh + 0x30)?, Kind::VertexBuffer)?;
        let index = buffer(self, source.u32(mesh + 0x40)?, Kind::IndexBuffer)?;
        let selected = index
            .data
            .get(..count.checked_mul(2).context("Particle draw overflow")?)
            .context("Source particle draw exceeds its index buffer")?;
        ensure!(
            selected
                .chunks_exact(2)
                .all(
                    |bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))
                        < vertex.data.len() / 32
                ),
            "Source particle draw references an absent vertex"
        );
        let material = source.u32(part)?;
        ensure!(
            self.reference(material)? == 0x80801C32,
            "Source particle model material class differs"
        );
        for at in (0x20..0x30).step_by(4).chain((mesh..mesh + 48).step_by(4)) {
            source.f32(at)?;
        }
        let mut out = Payload(vec![0; 160]);
        put(&mut out.0, 0x20, &source.bytes::<16>(0x20)?)?;
        put(&mut out.0, 0x30, &32u64.to_le_bytes())?;
        put(&mut out.0, 0x38, &0x0000030000000001u64.to_le_bytes())?;
        put(&mut out.0, 0x50, &source.bytes::<48>(mesh)?)?;
        put(&mut out.0, 0x90, &u32::MAX.to_le_bytes())?;
        // A conservative sphere bounds the complete transformed float stream. Keep the
        // source transform bits that the original particle draw publishes to render scope 8.
        let mut radius_squared = 0.0f64;
        for row in vertex.data.chunks_exact(32) {
            let mut squared = 0.0;
            for axis in 0..3 {
                let value = f32::from_le_bytes(row[axis * 4..axis * 4 + 4].try_into()?) as f64;
                let delta = value * f64::from(source.f32(mesh + axis * 4)?);
                squared += delta * delta;
            }
            radius_squared = radius_squared.max(squared);
        }
        let radius = radius_squared.sqrt() as f32;
        ensure!(radius.is_finite(), "Particle geometry bounds overflow");
        let radius = radius.next_up();
        ensure!(
            radius.is_finite(),
            "Particle geometry bounds exceed native precision"
        );
        put(&mut out.0, 0x80, &source.bytes::<12>(mesh + 16)?)?;
        for at in [0x20, 0x24, 0x8C] {
            put(&mut out.0, at, &radius.to_le_bytes())?;
        }
        append(&mut out.0, 16, 0x80807378, &[0; 136], 136)?;
        let target = out.array(16, 136, Some(0x80807378))?[0];
        for at in [0, 4, 8, 12, 16] {
            put(&mut out.0, target + at, &u32::MAX.to_le_bytes())?;
        }
        for stage in 1..24 {
            put(&mut out.0, target + 40 + stage * 2, &1u16.to_le_bytes())?;
        }
        for stage in 0..23 {
            put(&mut out.0, target + 88 + stage * 2, &13u16.to_le_bytes())?;
        }
        let mut draw = [0; 32];
        put(&mut draw, 0, &u32::MAX.to_le_bytes())?;
        put(&mut draw, 4, &source.bytes::<12>(part + 4)?)?;
        put(&mut draw, 16, &u32::try_from(count / 3)?.to_le_bytes())?;
        draw[28] = 127;
        draw[29] = 1;
        append(&mut out.0, target + 24, 0x8080737E, &draw, 32)?;
        let part = out.array(target + 24, 32, Some(0x8080737E))?[0];
        let references = vec![
            Reference {
                offset: target,
                source: vertex.source,
                kind: Kind::VertexBuffer,
            },
            Reference {
                offset: target + 16,
                source: index.source,
                kind: Kind::IndexBuffer,
            },
            Reference {
                offset: part,
                source: material,
                kind: Kind::Material,
            },
        ];
        self.unchanged()?;
        Ok(Geometry {
            bytes: out.0,
            references,
            buffers: vec![vertex, index],
        })
    }
}

fn buffer(source: &Source, tag: u32, kind: Kind) -> Result<Buffer> {
    let subtype = match kind {
        Kind::VertexBuffer => 4,
        Kind::IndexBuffer => 6,
        Kind::Material => anyhow::bail!("Particle material is not a buffer"),
    };
    let entry = source
        .package(tag)?
        .reader
        .entry(TagHash(tag).entry_index() as usize)
        .context("Source particle buffer header is absent")?;
    ensure!(
        entry.file_type == 32 && entry.file_subtype == subtype,
        "Source particle buffer header type differs"
    );
    let data_tag = entry.reference;
    let data_entry = source
        .package(data_tag)?
        .reader
        .entry(TagHash(data_tag).entry_index() as usize)
        .context("Source particle buffer data is absent")?;
    ensure!(
        data_entry.file_type == 1
            && data_entry.file_subtype == subtype
            && data_entry.reference == u32::MAX
            && data_entry.file_size <= 32 * 1024 * 1024,
        "Source particle raw buffer type, reference or extent differs"
    );
    let mut header = source.read(tag)?;
    if kind == Kind::VertexBuffer {
        ensure!(
            header.0.len() == 12 && header.u32(8)? == 0xBEEFDEAD,
            "Source particle vertex header marker differs"
        );
        put(&mut header.0, 8, &0xDEADBEEFu32.to_le_bytes())?;
    }
    let result = Buffer {
        source: tag,
        data_source: data_tag,
        kind,
        header: header.0,
        data: source.read(data_tag)?.0,
    };
    result.validate()?;
    Ok(result)
}
