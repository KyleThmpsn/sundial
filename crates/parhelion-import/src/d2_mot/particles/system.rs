//! Particle system envelopes. Dependency conversion is explicit and typed.
use super::*;

/// A completed dependency supplied by the native asset linker.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub tag: u32,
    pub class: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct System {
    pub program: u32,
    pub compute: [Option<u32>; 3],
    pub material: u32,
    pub model: Option<u32>,
    /// Serialized identity outside the package-tag range. Its runtime role is
    /// unresolved. Do not infer that it is a GPU binding merely from its value.
    pub gpu_binding: Option<u32>,
    pub render_layout: u32,
    pub draw_metadata: [u8; 16],
}

/// Renderer metadata produced for the target runtime. Source identifiers and
/// packed renderer fields cannot be passed through merely because they fit.
#[derive(Clone, Debug)]
pub struct NativeMetadata {
    pub render_layout: u32,
    pub draw_metadata: [u8; 16],
}

fn optional(value: u32) -> Option<u32> {
    (value != u32::MAX).then_some(value)
}

fn link(map: &BTreeMap<u32, Dependency>, source: u32, class: u32) -> Result<u32> {
    let value = map
        .get(&source)
        .with_context(|| format!("particle dependency {source:08X} is not translated"))?;
    ensure!(value.class == class, "particle dependency class differs");
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&value.tag),
        "particle dependency has an invalid native tag"
    );
    Ok(value.tag)
}

impl System {
    /// Read class 80806920. The resolver is only called for a non-null model
    /// reference and must resolve that exact encoded 64-bit source reference.
    pub fn read(bytes: &[u8], mut resolve_model: impl FnMut(u64) -> Result<u32>) -> Result<Self> {
        ensure!(bytes.len() == 72, "particle system envelope differs");
        let p = Payload(bytes.to_vec());
        ensure!(
            p.u32(4)? == u32::MAX,
            "source-only particle compute pass requires translation"
        );
        ensure!(p.u32(0x1C)? == 0, "particle system padding differs");
        let model = match (p.u32(0x20)?, p.u32(0x24)?, p.u64(0x28)?) {
            (u32::MAX, 1, 0) => None,
            (u32::MAX, 0, hash) if hash != 0 => Some(resolve_model(hash)?),
            (tag, 1, 0) if tag != u32::MAX => Some(tag),
            _ => bail!("particle model reference encoding differs"),
        };
        ensure!(
            p.u32(0x44)? == u32::MAX,
            "particle system trailing reference differs"
        );
        Ok(Self {
            program: p.u32(0)?,
            compute: [
                optional(p.u32(8)?),
                optional(p.u32(12)?),
                optional(p.u32(20)?),
            ],
            material: p.u32(24)?,
            model,
            gpu_binding: optional(p.u32(0x30)?),
            render_layout: p.u32(0x10)?,
            draw_metadata: p.bytes(0x34)?,
        })
    }

    /// Author class 80806E28 without a weapon donor. All package references
    /// must name completed native dependencies. Compiled identities use a
    /// separate map until their role and native representation are established.
    pub fn emit(
        &self,
        dependencies: &BTreeMap<u32, Dependency>,
        gpu_bindings: &BTreeMap<u32, u32>,
        point_model: Dependency,
        metadata: &NativeMetadata,
    ) -> Result<Vec<u8>> {
        ensure!(
            point_model.class == 0x80806E2E,
            "particle point model class differs"
        );
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&point_model.tag),
            "particle point model tag is invalid"
        );
        let mut output = vec![0; 52];
        let mut put =
            |at: usize, value: u32| output[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(0, link(dependencies, self.program, 0x80806E2C)?);
        for (source, offset) in self.compute.into_iter().zip([4, 8, 16]) {
            put(
                offset,
                source
                    .map(|tag| link(dependencies, tag, 0x808071E8))
                    .transpose()?
                    .unwrap_or(u32::MAX),
            );
        }
        put(12, metadata.render_layout);
        put(20, link(dependencies, self.material, 0x808071E8)?);
        put(
            24,
            self.model
                .map(|tag| link(dependencies, tag, 0x80806E2E))
                .transpose()?
                .unwrap_or(point_model.tag),
        );
        put(
            28,
            match self.gpu_binding {
                Some(source) => *gpu_bindings
                    .get(&source)
                    .context("particle compiled identity is not translated")?,
                None => u32::MAX,
            },
        );
        put(48, u32::MAX);
        output[32..48].copy_from_slice(&metadata.draw_metadata);
        Ok(output)
    }
}

/// Translate the class-80806929 container, preserving model order. Native
/// class 80806E2E keeps the same envelope and a class-808073A4 model array.
pub fn model_container(bytes: &[u8], dependencies: &BTreeMap<u32, Dependency>) -> Result<Vec<u8>> {
    let models = model_container_sources(bytes)?;
    let mut out = model_container_payload(&models);
    for (i, source) in models.into_iter().enumerate() {
        out[64 + i * 4..68 + i * 4]
            .copy_from_slice(&link(dependencies, source, 0x808073A5)?.to_le_bytes());
    }
    Ok(out)
}

/// Validate a source wrapper before allocating or linking any geometry nodes.
/// Duplicated models remain duplicated and their authored order is retained.
pub fn model_container_sources(bytes: &[u8]) -> Result<Vec<u32>> {
    let p = Payload(bytes.to_vec());
    ensure!(
        bytes.len() >= 32 && p.u64(0)? == bytes.len() as u64,
        "particle model container envelope differs"
    );
    ensure!(
        p.u32(8)? == u32::MAX && p.u32(12)? == 0,
        "particle model container header differs"
    );
    let models = p.array(16, 4, Some(0x80806F06))?;
    ensure!(
        models.len() <= 256,
        "particle model container exceeds capacity"
    );
    if models.is_empty() {
        ensure!(
            bytes.len() == 32 && p.u64(24)? == 0,
            "empty particle model container carries extra data"
        );
    } else {
        ensure!(
            p.pointer(24)? == 48
                && p.u32(44)? == 0x80809FB8
                && bytes[32..44].iter().all(|&value| value == 0)
                && bytes.len() == 64 + models.len() * 4,
            "particle model container carries an unsupported layout"
        );
    }
    models
        .into_iter()
        .map(|at| {
            let tag = p.u32(at)?;
            ensure!(
                (0x80800001..=0x81FFFFFF).contains(&tag),
                "particle model source tag is invalid"
            );
            Ok(tag)
        })
        .collect()
}

pub(super) fn model_container_payload(models: &[u32]) -> Vec<u8> {
    let mut out = vec![
        0;
        if models.is_empty() {
            32
        } else {
            64 + models.len() * 4
        }
    ];
    let size = out.len() as u64;
    out[..8].copy_from_slice(&size.to_le_bytes());
    out[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    out[16..24].copy_from_slice(&(models.len() as u64).to_le_bytes());
    if !models.is_empty() {
        out[24..32].copy_from_slice(&24i64.to_le_bytes());
        out[44..48].copy_from_slice(&0x80809FBDu32.to_le_bytes());
        out[48..56].copy_from_slice(&(models.len() as u64).to_le_bytes());
        out[56..60].copy_from_slice(&0x808073A4u32.to_le_bytes());
        for i in 0..models.len() {
            out[64 + i * 4..68 + i * 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    out
}
