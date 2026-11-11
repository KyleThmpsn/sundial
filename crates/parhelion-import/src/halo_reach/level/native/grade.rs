//! Scenario display colour through Shadowkeep's native screen-effect volume and lookup table.
//! Reach's global `sefc` rows accumulate at full weight before its final gamma and colour matrix.
//! Shadowkeep applies its film curve before a 32-cube grade, so the table maps that film output
//! back through the source display response. Its finite grid limits dark-end accuracy.
use crate::{
    halo_reach::cache::Cache,
    presentation::{Graph, append, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const EFFECT: u32 = 0x80C0_03A5;
const EFFECT_CLASS: u32 = 0x8080_707E;
const PARAMETERS: u32 = 0x80C7_0F1D;
const PARAMETERS_CLASS: u32 = 0x8080_71DC;
const TEXTURE: u32 = 0x80C7_0C73;
const SIZE: usize = 32;
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Shadowkeep's ordinary final film response, before the native colour lookup.
fn film(x: f64) -> f64 {
    let a =
        ((4.016 * x + 0.03) * (1.6 * x) / ((3.888 * x + 0.59) * (1.6 * x) + 0.14)).clamp(0., 1.);
    let b =
        (x * (1.048747 * x + 3.134397) / (x * (0.990440 * x + 3.240450) + 0.651790)).clamp(0., 1.);
    0.4 * a + 0.6 * b
}

fn inverse(value: f64) -> f64 {
    if value == 0. {
        return 0.;
    }
    let (mut low, mut high) = (0., 64.);
    for _ in 0..60 {
        let middle = (low + high) * 0.5;
        if film(middle) < value {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) * 0.5
}

struct Source {
    gamma: f64,
    saturation: f64,
    contrast: f64,
    filter: [f64; 3],
    floor: [f64; 3],
    report: Value,
}

impl Source {
    fn read(cache: &Cache) -> Result<Option<Self>> {
        let scenario = cache.only("scnr")?.address()?;
        let tag = cache.reference(scenario + 0x6B8, Some("sefc"))?;
        let mut effects = [0f32; 11];
        let mut filter = [1f32; 3];
        let mut floor = [0f32; 3];
        let mut rows = 0;
        if let Some(tag) = &tag {
            let at = tag.address()?;
            if cache.u16(at + 2)? & 2 != 0 {
                for row in cache.block(at + 4, 0xF0)? {
                    // Original global accumulation admits render-thread rows and supplies unit
                    // weight directly. Local effect falloffs and visibility are a separate path.
                    if cache.u16(row + 6)? & 2 == 0 {
                        continue;
                    }
                    let values = cache.floats::<11>(row + 0x6C)?;
                    let color = cache.floats::<3>(row + 0x98)?;
                    let lower = cache.floats::<3>(row + 0xA4)?;
                    ensure!(
                        values
                            .iter()
                            .chain(color.iter())
                            .chain(lower.iter())
                            .all(|v| v.is_finite()),
                        "Scenario screen effects contain nonfinite display settings"
                    );
                    for (target, value) in effects.iter_mut().zip(values) {
                        *target = target.max(value);
                    }
                    for k in 0..3 {
                        filter[k] = filter[k].min(color[k]);
                        floor[k] = floor[k].max(lower[k]);
                    }
                    rows += 1;
                }
            }
        }
        // Hue rotation has not been qualified against the source matrix consumer. Keep the
        // existing native display path for that structural case rather than inventing a matrix.
        if effects[2] != 0. || effects[3] != 0. {
            return Ok(None);
        }
        ensure!(
            filter.iter().all(|v| *v >= 0.),
            "Negative scenario colour filter"
        );
        let gamma = 0.5 * f64::from((1. + effects[7] - effects[8]).clamp(0.01, 10.));
        let saturation = f64::from((1. + effects[4] - effects[5]).clamp(-10., 10.));
        let contrast = f64::from(effects[6]);
        Ok(Some(Self {
            gamma,
            saturation,
            contrast,
            filter: filter.map(|v| f64::from(v).sqrt()),
            floor: floor.map(f64::from),
            report: json!({
                "source": tag.map(|tag| tag.path), "render_rows": rows,
                "gamma": gamma, "saturation": saturation, "contrast": contrast,
                "filter": filter, "floor": floor,
                "exposure_boost": effects[0], "exposure_deboost": effects[1],
                "limits": "Static scenario gamma and colour matrix only. Source exposure boost, local and scripted effects, noise and screen shaders retain native behavior. The native 32-cube interpolates dark values and source highlights clip.",
            }),
        }))
    }

    fn color(&self, scene: [f64; 3]) -> [f64; 3] {
        let gamma = scene.map(|v| v.powf(self.gamma));
        let grey = gamma.iter().zip(LUMA).map(|(v, w)| v * w).sum::<f64>();
        let contrast = self.contrast * self.contrast;
        std::array::from_fn(|k| {
            let saturated = grey + self.saturation * (gamma[k] - grey);
            (saturated * self.filter[k] * (1. + 4. * contrast) - self.floor[k] - contrast)
                .clamp(0., 1.)
        })
    }
}

fn program(template: &Payload, rows: &[[f32; 4]]) -> Result<Vec<u8>> {
    let mut data = template.0[..0x90].to_vec();
    data[0x10..0x80].fill(0);
    append(
        &mut data,
        0x60,
        0x8080_0090,
        &rows
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        16,
    )?;
    Ok(data)
}

pub(super) fn add(
    graph: &mut Graph,
    native: &mut Reader,
    cache: &Cache,
) -> Result<(Option<String>, Value)> {
    let Some(source) = Source::read(cache)? else {
        return Ok((
            None,
            json!({"translated": false, "reason": "Scenario hue rotation is not yet translated"}),
        ));
    };
    let template = native.tag(PARAMETERS, Some(PARAMETERS_CLASS))?;
    let rows = template.array(0x60, 16, Some(0x8080_0090))?;
    ensure!(
        template.0.len() == 0x180 && rows.len() == 13,
        "Screen parameter layout differs"
    );
    let mut settings = [[0f32; 4]; 13];
    for (target, at) in settings.iter_mut().zip(rows) {
        for (k, value) in target.iter_mut().enumerate() {
            *value = template.f32(at + 4 * k)?;
        }
    }
    let weight = program(&template, &[[1., 1., 1., 0.]])?;
    let settings = program(&template, &settings)?;
    let mut effect = native.tag(EFFECT, Some(EFFECT_CLASS))?.0.clone();
    ensure!(effect.len() == 0x50, "Screen effect layout differs");
    // Environment instances have no attached view. The donor's view-ID restriction would
    // discard their grade before evaluating its weight and settings.
    effect[0x0D] &= !0x04;
    // No spherical or directional falloff. The enclosing native environment volume owns weight.
    effect[0x10..0x28].fill(0);
    for field in [0x28, 0x3C, 0x40] {
        put(&mut effect, field, &u32::MAX.to_le_bytes())?;
    }
    let mut header = native.tag(TEXTURE, None)?.0.clone();
    let check = Payload(header.clone());
    ensure!(
        header.len() == 40
            && check.u32(4)? == 28
            && check.u16(14)? == 1024
            && check.u16(16)? == 32
            && check.u16(18)? == 1
            && check.u16(20)? == 1
            && check.u8(23)? == 1,
        "Native colour lookup is not a single-mip 1024 by 32 linear RGBA table"
    );
    let data_template = native.reference(TEXTURE)?;
    let linear: [f64; SIZE] = std::array::from_fn(|i| inverse(i as f64 / (SIZE - 1) as f64));
    let mut data = Vec::with_capacity(SIZE * SIZE * SIZE * 4);
    // Stock 2D colour lookups store red along each tile, blue across tiles and green by row.
    for green in linear {
        for blue in linear {
            for red in linear {
                data.extend(
                    source
                        .color([red, green, blue])
                        .map(|v| (255. * v).round() as u8),
                );
                data.push(255);
            }
        }
    }
    crate::tiger::texture::resident(&mut header, data.len())?;
    graph.add(
        "display-grade-data",
        data_template,
        &data,
        Some("display-grade-texture"),
        Vec::new(),
    )?;
    graph.add(
        "display-grade-texture",
        TEXTURE,
        &header,
        Some("display-grade-data"),
        Vec::new(),
    )?;
    graph.add(
        "display-grade-weight",
        PARAMETERS,
        &weight,
        None,
        Vec::new(),
    )?;
    graph.add(
        "display-grade-settings",
        PARAMETERS,
        &settings,
        None,
        Vec::new(),
    )?;
    graph.add(
        "display-grade",
        EFFECT,
        &effect,
        None,
        vec![
            json!({"offset": 0x28, "symbol": "display-grade-texture"}),
            json!({"offset": 0x3C, "symbol": "display-grade-weight"}),
            json!({"offset": 0x40, "symbol": "display-grade-settings"}),
        ],
    )?;
    let mut report = source.report;
    report["translated"] = json!(true);
    report["native_table_size"] = json!([1024, 32]);
    report["native_volume_type"] = json!(6);
    Ok((Some("display-grade".into()), report))
}
