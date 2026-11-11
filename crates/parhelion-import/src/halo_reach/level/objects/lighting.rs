//! Persistent scenery probes in MCC U13 lightmap BSP data. These keep their world-space
//! direction and analytical selection when an unchanged scenario placement becomes static
//! geometry. Moved, parented and explicitly posed placements keep the spatial fallback.
use super::*;

/// MCC's scenery-probe block, distinct from the instance colorings at `Lbsp+0x6C`.
const PROBES: usize = 0x138;
const PROBE_SIZE: usize = 0x2C;
const IDENTITY: usize = 0x3C;

struct Probe {
    row: usize,
    bsp: String,
}

pub struct Probes {
    rows: BTreeMap<[u8; 8], Probe>,
    pub report: Report,
}

#[derive(Default, serde::Serialize)]
pub struct Report {
    source_rows: usize,
    applied: Vec<Applied>,
    skipped: Vec<Skipped>,
}

#[derive(serde::Serialize)]
struct Applied {
    identity: String,
    object: String,
    bsp: String,
    analytical_light: u32,
    primitives: usize,
}

#[derive(serde::Serialize)]
struct Skipped {
    identity: String,
    reason: &'static str,
}

impl Probes {
    pub fn read(cache: &Cache, loaded: u32) -> Result<Self> {
        let mut rows = BTreeMap::new();
        let scenario = cache.only("scnr")?.address()?;
        for (index, source) in cache.block(scenario + 0x50, 0xAC)?.into_iter().enumerate() {
            if index >= 32 || loaded & (1 << index) == 0 {
                continue;
            }
            let Some(bsp) = cache.reference(source, Some("sbsp"))? else {
                continue;
            };
            let Some(lightmap) = cache
                .tags
                .iter()
                .flatten()
                .find(|tag| tag.group == super::super::LIGHTMAP_GROUP && tag.path == bsp.path)
            else {
                continue;
            };
            for row in cache.block(lightmap.address()? + PROBES, PROBE_SIZE)? {
                let identity: [u8; 8] = cache.bytes(row, 8)?.try_into()?;
                ensure!(
                    rows.insert(
                        identity,
                        Probe {
                            row,
                            bsp: bsp.path.clone()
                        }
                    )
                    .is_none(),
                    "Duplicate scenery lighting identity in {}",
                    lightmap.path
                );
            }
        }
        Ok(Self {
            report: Report {
                source_rows: rows.len(),
                ..Report::default()
            },
            rows,
        })
    }

    pub fn apply(
        &mut self,
        cache: &Cache,
        placement: usize,
        object: &Tag,
        posed: bool,
        primitives: &mut [Primitive],
    ) -> Result<()> {
        if primitives.is_empty() {
            return Ok(());
        }
        let identity: [u8; 8] = cache.bytes(placement + IDENTITY, 8)?.try_into()?;
        let Some(probe) = self.rows.get(&identity) else {
            return Ok(());
        };
        let identity = identity
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let model = cache.reference(object.address()? + 0x64, Some("hlmt"))?;
        let policy = model
            .map(|model| cache.u32(model.address()? + 0x15C))
            .transpose()?
            .unwrap_or(0);
        let reason = if posed {
            Some("The scripted pose differs from the baked placement")
        } else if cache.i16(placement + 0x4A)? >= 0 {
            Some("The placement is attached to a parent")
        } else if policy & 0x1C != 0 {
            // The inspected consumer has separate sky, air-probe and locked-probe routes.
            // Only the ordinary scenery route is translated here.
            Some("The model selects a non-default lighting policy")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.report.skipped.push(Skipped { identity, reason });
            return Ok(());
        }
        let bytes = cache.bytes(probe.row + 8, 32)?;
        let h = std::array::from_fn::<f32, 16, _>(|index| {
            half::f16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]).to_f32()
        });
        ensure!(
            h.iter().all(|v| v.is_finite()),
            "Nonfinite scenery probe {identity}"
        );
        ensure!(
            [3, 7, 11, 15]
                .into_iter()
                .all(|index| (0. ..=1.).contains(&h[index]))
                && [4, 5, 6, 12, 13, 14]
                    .into_iter()
                    .all(|index| h[index] >= 0.),
            "Invalid scenery probe color, bandwidth or visibility {identity}"
        );
        if h[8..11].iter().any(|&v| v != 0.) || h[15] != 0. {
            self.report.skipped.push(Skipped {
                identity,
                reason: "The second probe lobe is directional",
            });
            return Ok(());
        }
        let direction = [h[0], h[1], h[2]];
        let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        let [x, y, z] = if length > 1e-5 {
            direction.map(|v| v / length)
        } else {
            [0., 0., 1.]
        };
        // Like instance colorings, the half probe holds irradiance. The world material's
        // lighting inputs are irradiance over pi, with the ambient quarter applied. Vertex
        // records carry squared analytical visibility, while lightmap images carry its root.
        let scale = 1. / std::f32::consts::PI;
        let lighting = [
            [h[4] * scale, h[5] * scale, h[6] * scale, h[7]],
            [
                h[12] * scale * 0.25,
                h[13] * scale * 0.25,
                h[14] * scale * 0.25,
                h[3] * h[3],
            ],
            [x, y, z, 1.],
        ];
        let analytical_light = cache.u32(probe.row + 0x28)?;
        for primitive in primitives.iter_mut() {
            primitive.analytical_light = Some(analytical_light);
            for vertex in &mut primitive.vertices {
                vertex.lighting = Some(lighting);
            }
        }
        self.report.applied.push(Applied {
            identity,
            object: object.path.clone(),
            bsp: probe.bsp.clone(),
            analytical_light,
            primitives: primitives.len(),
        });
        Ok(())
    }
}
