//! Analytical-light selection in MCC 1.3528.0.0, separate from baked VMF lighting.
//! The lookup and constant setter both matter: identifier zero resolves a sky in the lookup,
//! but the setter disables its intensity. Source records do not establish animated intensity,
//! gel sampling or the target renderer's shadow behavior.
use super::*;

pub(super) const SKY: u32 = 0xffbeef00;

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Analytical {
    pub identifier: u32,
    /// Direction toward a directional light, or a point light's position in Reach world units.
    pub vector: [f32; 3],
    pub intensity: [f32; 3],
    pub directional: bool,
}

impl Analytical {
    fn checked(self) -> Result<Self> {
        ensure!(
            self.vector
                .iter()
                .chain(&self.intensity)
                .all(|v| v.is_finite()),
            "Nonfinite analytical light {:08X}",
            self.identifier
        );
        ensure!(
            self.intensity.iter().all(|v| *v >= 0.),
            "Negative analytical intensity"
        );
        Ok(self)
    }

    pub(in crate::halo_reach) fn present(self) -> bool {
        self.intensity.iter().any(|v| *v > 0.)
            && (!self.directional || self.vector.iter().map(|v| v * v).sum::<f32>() > 1e-10)
    }
}

pub(super) fn read(cache: &Cache, identifier: u32) -> Result<Analytical> {
    let scenario = cache.only("scnr")?.address()?;
    if identifier != 0 && identifier & SKY != SKY {
        let bsp_index = (!(identifier >> 24) & 0xff) as usize;
        let instance_index = (identifier & 0x00ff_ffff) as usize;
        let bsps = cache.block(scenario + SCENARIO_BSPS, SCENARIO_BSP_SIZE)?;
        if let Some(&bsp) = bsps.get(bsp_index)
            && let Some(tag) = cache.reference(bsp + 0x20, Some("stli"))?
        {
            let at = tag.address()?;
            let instances = cache.block(at + 0x10, 0x84)?;
            if let Some(&instance) = instances.get(instance_index) {
                let definitions = cache.block(at + 4, 0x50)?;
                let index = usize::try_from(cache.i32(instance)?)?;
                let definition = *definitions
                    .get(index)
                    .context("Analytical light definition outside stli")?;
                let directional = cache.i16(definition)? == 2;
                let color = cache.floats::<3>(definition + 8)?;
                let intensity = cache.f32(definition + 0x14)?;
                return Analytical {
                    identifier,
                    vector: cache.floats::<3>(instance + if directional { 0x20 } else { 8 })?,
                    intensity: color.map(|v| v.clamp(0., 1.).powf(2.2) * intensity),
                    directional,
                }
                .checked();
            }
        }
    }
    let skies = cache.block(scenario + 0x88, 0x30)?;
    let model = if skies.is_empty() {
        None
    } else {
        let row = skies[(identifier as u8 as usize) % skies.len()];
        match cache.reference(row, Some("scen"))? {
            Some(object) => match cache.reference(object.address()? + 0x64, Some("hlmt"))? {
                Some(model) => cache.reference(model.address()?, Some("mode"))?,
                None => None,
            },
            None => None,
        }
    };
    // The original MCC fallback constant, independently read from the mapped consumer's data.
    // Missing chains use this value. A present sky with zero intensity remains zero.
    #[allow(
        clippy::approx_constant,
        reason = "Preserve the original MCC fallback's stored float values"
    )]
    let (vector, intensity) = match model {
        Some(model) => (
            cache.floats::<3>(model.address()? + 0x224)?,
            cache.floats::<3>(model.address()? + 0x230)?,
        ),
        None => (
            [-0.1227878, -0.6963643, 0.7071068],
            [4.659558, 3.947189, 3.600812],
        ),
    };
    Analytical {
        identifier,
        vector,
        intensity: if identifier == 0 { [0.; 3] } else { intensity },
        directional: true,
    }
    .checked()
}

/// Split material identities by source selection before native triangles are grouped.
pub(super) fn materials(cache: &Cache, level: &mut Level) -> Result<BTreeMap<u32, Analytical>> {
    let mut variants = BTreeMap::new();
    let mut lights = BTreeMap::new();
    let mut selected = BTreeMap::new();
    for primitive in &mut level.primitives {
        // Model placements have no structure subpart ID. Sky zero is the current static
        // adaptation until object cluster/probe selection is translated.
        let identifier = primitive.analytical_light.unwrap_or(SKY);
        let key = (primitive.material, identifier);
        let material = match variants.get(&key) {
            Some(&index) => index,
            None => {
                let source = primitive
                    .material
                    .and_then(|index| level.materials.get(index))
                    .cloned()
                    .unwrap_or(Material {
                        decal: None,
                        tag: None,
                        options: BTreeMap::new(),
                        properties: Vec::new(),
                    });
                let index = level.materials.len();
                level.materials.push(source);
                let light = match selected.get(&identifier) {
                    Some(&light) => light,
                    None => {
                        let light = read(cache, identifier)?;
                        selected.insert(identifier, light);
                        light
                    }
                };
                lights.insert(u32::try_from(index)?, light);
                variants.insert(key, index);
                index
            }
        };
        primitive.material = Some(material);
    }
    Ok(lights)
}
