//! The supercombine finish's native light, an effect-only projectile's flight light and a contact
//! host's lights, in a source light's color.
//!
//! The stock finish `80EF27FB` lights its detonation with record `8152900D` (class `80807140`). The
//! record names its material at `+0x90` and its primary and supplemental parameter records (class
//! `808071DC`) at `+0xA0` and `+0xA4`. The material's pixel program `815B90D2` shades with
//! `lerp(row 11, row 9 * row 10.x, saturate(row 8.y * attenuation + row 8.x))` plus row 13 times its
//! diffuse and specular terms. These are rows of its initial pixel constants, teal at 11 and 13 and
//! blue at 9, and its pixel expression writes none of them. The primary record's initial vectors 2
//! and 3 and the supplemental record's vectors 0 and 1 repeat the row 13 and row 11 colors. A
//! private copy writes all of them with the source light's hue and saturation at each donor color's
//! own luminance, so the light keeps the donor's brightness. It carries its programs' data
//! entries, since a container that loads only a header leaves its data unloaded.
//!
//! The record's reach is a volume transform at `+0x20`, four rows whose diagonal holds the
//! negated half extent, the half extent at `+0x6C` and `+0x80` and a bounding radius, `sqrt(3)`
//! times it, at `+0x7C`. Stock lights `8152900D` and `815B39AE` store 2.5 and 4.33 metres there. A
//! source cheap light (`gldf`) gives the copy its own radius in all of them. That these fields
//! bound the lit volume is inferred from their values.
use super::bolt::{matched, paired};
use super::*;

/// The stock finish's light record.
pub(in crate::halo_reach) const TEMPLATE: u32 = 0x8152_900d;
const PIXEL: u32 = 0x815b_90d2;
const LIGHT_CLASS: u32 = 0x8080_7140;
const PARAMETERS_CLASS: u32 = 0x8080_71dc;
const MATERIAL_CLASS: u32 = 0x8080_71e8;
const VECTORS: u32 = 0x8080_0090;
const STAGES: [usize; 6] = [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368];
const PIXEL_CONSTANTS: usize = 0x2c8 + 0x50;
/// The pixel constant rows holding the far, near and lit colors.
const COLORS: [usize; 3] = [9, 11, 13];
/// Each parameter record's field in the light record, and its vectors that repeat the lit and
/// near colors.
const PARAMETERS: [(usize, [usize; 2]); 2] = [(0xa0, [2, 3]), (0xa4, [0, 1])];
const SYMBOL: &str = "supercombine-light";
/// The stock flow whose light event names [`TEMPLATE`], the supercombine finish's.
pub(in crate::halo_reach) const EVENT_FLOW: u32 = 0x80ef_27f9;

/// The checked private copies, before any is placed.
pub(in crate::halo_reach) struct Template {
    light: Payload,
    material: (u32, Payload),
    parameters: Vec<(usize, u32, Payload)>,
    donor: Vec<[f32; 3]>,
    written: Vec<[f32; 3]>,
    radius: Option<f32>,
}

/// A source light part or attachment: its linear color, the seconds it lasts, zero for no limit,
/// and its radius in metres where the source gives one.
pub(in crate::halo_reach) struct Source {
    pub color: [f32; 3],
    pub seconds: f32,
    pub radius: Option<f32>,
}

/// A source light (`ligh`), which keeps the native record's reach, or cheap light (`gldf`), which
/// lasts as long as its effect event and gives its own radius.
pub(in crate::halo_reach) fn source(cache: &Cache, tag: &Tag, metres: f32) -> Result<Source> {
    match tag.group.as_str() {
        "ligh" => {
            let (color, seconds) = crate::halo_reach::level::native::lights::color(cache, tag)?;
            Ok(Source {
                color,
                seconds,
                radius: None,
            })
        }
        "gldf" => {
            let (color, radius) = crate::halo_reach::level::native::lights::cheap(cache, tag)?;
            Ok(Source {
                color,
                seconds: 0.,
                radius: Some(radius * metres),
            })
        }
        group => anyhow::bail!("Source {group} tags are not lights"),
    }
}

fn rgb(p: &Payload, row: usize) -> Result<[f32; 3]> {
    Ok([p.f32(row)?, p.f32(row + 4)?, p.f32(row + 8)?])
}

fn write(p: &mut Payload, row: usize, rgb: [f32; 3]) -> Result<()> {
    for (lane, value) in rgb.into_iter().enumerate() {
        ensure!(value.is_finite(), "Light color is not finite");
        put(&mut p.0, row + lane * 4, &value.to_le_bytes())?;
    }
    Ok(())
}

/// Private copies of the finish light recolored to linear `color`. Fails without placing
/// anything when the stock light is laid out another way.
pub(in crate::halo_reach) fn template(native: &mut Reader, color: [f32; 3]) -> Result<Template> {
    ensure!(
        color.iter().all(|v| v.is_finite())
            && 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2] > 1e-3,
        "Source detonation light is black"
    );
    let mut light = (*native.tag(TEMPLATE, Some(LIGHT_CLASS))?).clone();
    let material_tag = light.u32(0x90)?;
    let mut material = (*native.tag(material_tag, Some(MATERIAL_CLASS))?).clone();
    ensure!(
        material.u32(0x2c8)? == PIXEL,
        "Finish light shades with another program"
    );
    for stage in STAGES {
        ensure!(
            material.array(stage + 8, 8, Some(0x8080_7211))?.is_empty()
                && material.array(stage + 0x40, 16, None)?.is_empty()
                && matches!(material.u32(stage + 0x84)?, 0 | u32::MAX | 0x811c_9dc5),
            "Finish light material binds its own textures or buffers"
        );
    }
    let constants = material.array(PIXEL_CONSTANTS, 16, Some(VECTORS))?;
    let mut donor = Vec::new();
    let mut written = Vec::new();
    for row in COLORS {
        let at = *constants.get(row).context("Finish light color row")?;
        let original = rgb(&material, at)?;
        let recolored = matched(color, original)?;
        write(&mut material, at, recolored)?;
        donor.push(original);
        written.push(recolored);
    }
    // The near and lit colors, rows 11 and 13, in the order the parameter records hold them.
    let repeated = [donor[2], donor[1]];
    let mut parameters = Vec::new();
    for (field, vectors) in PARAMETERS {
        let tag = light.u32(field)?;
        let mut p = (*native.tag(tag, Some(PARAMETERS_CLASS))?).clone();
        let rows = p.array(0x60, 16, Some(VECTORS))?;
        for (vector, expected) in vectors.into_iter().zip(repeated) {
            let at = *rows.get(vector).context("Finish light parameter vector")?;
            ensure!(
                rgb(&p, at)? == expected,
                "Finish light parameters differ from its material"
            );
            write(&mut p, at, matched(color, expected)?)?;
        }
        put(&mut light.0, field, &u32::MAX.to_le_bytes())?;
        parameters.push((field, tag, p));
    }
    put(&mut light.0, 0x90, &u32::MAX.to_le_bytes())?;
    Ok(Template {
        light,
        material: (material_tag, material),
        parameters,
        donor,
        written,
        radius: None,
    })
}

impl Template {
    /// Gives the copy `radius` metres of reach, where given, in each field that stores the stock
    /// record's.
    pub(in crate::halo_reach) fn reach(mut self, radius: Option<f32>) -> Result<Self> {
        let Some(radius) = radius else {
            return Ok(self);
        };
        ensure!(
            radius.is_finite() && radius > 0.,
            "Source light radius is not positive"
        );
        let p = &mut self.light;
        let extent = p.f32(0x80)?;
        ensure!(
            extent > 0.
                && p.f32(0x6c)? == extent
                && [0x20, 0x34, 0x48]
                    .into_iter()
                    .all(|at| p.f32(at).ok() == Some(-extent))
                && [0x24, 0x28, 0x30, 0x38, 0x40, 0x44]
                    .into_iter()
                    .all(|at| p.f32(at).ok() == Some(0.))
                && (p.f32(0x7c)? - extent * 3f32.sqrt()).abs() <= extent * 1e-3,
            "Finish light volume differs"
        );
        for at in [0x20, 0x34, 0x48] {
            put(&mut p.0, at, &(-radius).to_le_bytes())?;
        }
        for at in [0x6c, 0x80] {
            put(&mut p.0, at, &radius.to_le_bytes())?;
        }
        put(&mut p.0, 0x7c, &(radius * 3f32.sqrt()).to_le_bytes())?;
        self.radius = Some(radius);
        Ok(self)
    }

    /// Places the copies and returns the private light, shown for `seconds`, with its report.
    pub(super) fn add(
        self,
        native: &mut Reader,
        graph: &mut Graph,
        seconds: f32,
    ) -> Result<(Light, Value)> {
        self.add_named(native, graph, seconds, SYMBOL)
    }

    /// As [`Self::add`], with the private copies named under `symbol`.
    pub(in crate::halo_reach) fn add_named(
        self,
        native: &mut Reader,
        graph: &mut Graph,
        seconds: f32,
        symbol: &str,
    ) -> Result<(Light, Value)> {
        let (template, mut material) = self.material;
        let mut patches = Vec::new();
        for stage in STAGES {
            let program = material.u32(stage)?;
            if program == u32::MAX {
                continue;
            }
            let name = paired(
                native,
                graph,
                &format!("{symbol}-program-{program:08X}"),
                program,
                None,
            )?;
            put(&mut material.0, stage, &u32::MAX.to_le_bytes())?;
            patches.push(json!({"offset":stage,"symbol":name}));
        }
        let name = format!("{symbol}-material");
        graph.add(&name, template, &material.0, None, patches)?;
        let mut patches = vec![json!({"offset":0x90,"symbol":name})];
        for (field, tag, p) in &self.parameters {
            let name = format!("{symbol}-parameters-{field:02X}");
            graph.add(&name, *tag, &p.0, None, vec![])?;
            patches.push(json!({"offset":field,"symbol":name}));
        }
        graph.add(symbol, TEMPLATE, &self.light.0, None, patches)?;
        Ok((
            Light {
                symbol: symbol.into(),
                template: TEMPLATE,
                seconds,
            },
            json!({"template":format!("{TEMPLATE:08X}"),"rows":COLORS,
                "donor_colors":self.donor,"colors":self.written,"radius_metres":self.radius}),
        ))
    }
}
