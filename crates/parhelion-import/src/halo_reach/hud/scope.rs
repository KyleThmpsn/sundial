//! Resolve scoped artwork through the weapon's MCC HUD, including atlas sprites.
use super::super::{
    cache::{Cache, Tag},
    material::Mapping,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub(crate) struct Layer {
    pub name: String,
    pub mapping: Mapping,
    pub uv: [f32; 4],
    pub size: [f32; 2],
    pub origin: [f32; 2],
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    pub flags: u32,
    pub shader: u8,
    pub opacity: f32,
    pub color: [[f32; 3]; 3],
}

#[derive(Serialize)]
pub(crate) struct Scope {
    pub hud: Tag,
    pub zoom_levels: u16,
    pub magnification: [f32; 2],
    pub layers: Vec<Layer>,
    pub excluded: Vec<Value>,
}

fn zoom(c: &Cache, weapon: &Tag) -> Result<(u16, [f32; 2])> {
    ensure!(weapon.group == "weap", "Zoom owner is not a weapon");
    let base = weapon.address()?;
    let levels = c.u16(base + 0x2ca)?;
    let magnification = c.floats::<2>(base + 0x2cc)?;
    ensure!(
        levels <= 2
            && magnification[..usize::from(levels)]
                .iter()
                .all(|value| (1.0..=10.0).contains(value)),
        "Unsupported source zoom range"
    );
    Ok((levels, magnification))
}

pub(crate) fn aim(c: &Cache, weapon: &Tag) -> Result<Value> {
    let (levels, magnification) = zoom(c, weapon)?;
    Ok(json!({
        "zoom_levels":levels,
        "source_magnification":&magnification[..usize::from(levels)],
        "magnification":if levels == 0 { 1.0 } else { magnification[0] },
        "method":"private_weapon_controller_zoom_translation",
        "limits":if levels > 1 {
            vec!["The first source zoom level is selected. Switching to additional source zoom levels is not translated."]
        } else { Vec::<&str>::new() },
        "gameplay_verified":false,
    }))
}

// MCC's active groups are ANDs of ORs. A scope can accept either zoom level,
// but must belong to the right-hand weapon. Other activation domains stay out.
fn state(c: &Cache, widget: usize, require_zoom: bool) -> Result<bool> {
    let rows = c.block(widget + 0x1c, 0x38)?;
    ensure!(rows.len() <= 1, "HUD widget has multiple state records");
    let mut zoom = false;
    for row in rows {
        if !c.block(row + 12, 12)?.is_empty() || !c.block(row + 24, 12)?.is_empty() {
            return Ok(false);
        }
        for group in c.block(row, 12)? {
            let terms = c.block(group, 8)?;
            ensure!(!terms.is_empty(), "HUD activation has an empty term group");
            let mut active = false;
            for term in terms {
                if c.u32(term)? != 0 {
                    continue;
                }
                match c.u32(term + 4)? {
                    85 => {
                        zoom = true;
                        active = true;
                    }
                    86 => zoom = true,
                    97 => active = true,
                    _ => {}
                }
            }
            if !active {
                return Ok(false);
            }
        }
    }
    Ok(!require_zoom || zoom)
}

fn placement(c: &Cache, widget: usize) -> Result<[f32; 6]> {
    let rows = c.block(widget + 0x38, 0x1c)?;
    if rows.is_empty() {
        return Ok([0.5, 0.5, 0., 0., 1., 1.]);
    }
    let row = *rows
        .first()
        .context("HUD widget has no full-screen placement")?;
    ensure!(
        c.u8(row)? <= 1 && matches!(c.u8(row + 1)?, 0 | 17) && c.u16(row + 2)? == 0,
        "Scope placement is not an unrotated parent or crosshair anchor"
    );
    let values = c.floats::<6>(row + 4)?;
    ensure!(
        values.iter().all(|v| v.is_finite()) && values[4..].iter().all(|v| v.abs() > 0.0001),
        "Degenerate scope placement"
    );
    Ok(values)
}

fn colors(c: &Cache, render: usize, globals: usize) -> Result<[[f32; 3]; 3]> {
    let mut result = [[0.; 3]; 3];
    for (i, color) in result.iter_mut().enumerate() {
        let selector = usize::from(c.u16(render + 0x54 + i * 2)?);
        *color = match selector {
            0..=3 => c.floats::<3>(render + 8 + selector * 16)?,
            18..=49 => c.floats::<3>(globals + 8 + (selector - 18) * 16)?,
            _ => anyhow::bail!("Unsupported scope color selector {selector}"),
        };
        ensure!(
            color.iter().all(|v| v.is_finite() && *v >= 0.),
            "Invalid scope color"
        );
    }
    Ok(result)
}

fn layer(c: &Cache, widget: usize, parent: [f32; 6], globals: usize) -> Result<Layer> {
    let bitmap = c
        .reference(widget + 0x80, Some("bitm"))?
        .context("Scope bitmap is missing")?;
    let sequence = usize::from(c.u8(widget + 0x90)?);
    let sequences = c.block(bitmap.address()? + 0x70, 0x40)?;
    let (frame, uv) = if sequences.is_empty() {
        (sequence, [0., 1., 0., 1.])
    } else {
        let sequence = *sequences
            .get(sequence)
            .context("Scope sequence is outside its bitmap")?;
        let sprites = c.block(sequence + 0x34, 0x20)?;
        if let [sprite] = sprites.as_slice() {
            (
                usize::try_from(c.i16(*sprite)?)?,
                c.floats::<4>(*sprite + 8)?,
            )
        } else {
            ensure!(
                sprites.is_empty() && c.i16(sequence + 0x22)? == 1,
                "Scope sequence requires animated sprite selection"
            );
            (usize::try_from(c.i16(sequence + 0x20)?)?, [0., 1., 0., 1.])
        }
    };
    ensure!(
        uv.iter().all(|v| (0.0..=1.).contains(v)) && uv[0] < uv[1] && uv[2] < uv[3],
        "Invalid scope sprite rectangle"
    );
    let images = c.block(bitmap.address()? + 0x7c, 0x38)?;
    let image = *images.get(frame).context("Scope bitmap frame is absent")?;
    let size = [
        f32::from(c.u16(image)?) * (uv[1] - uv[0]),
        f32::from(c.u16(image + 2)?) * (uv[3] - uv[2]),
    ];
    let p = placement(c, widget)?;
    let render = c.block(widget + 0x70, 0x78)?;
    let [render] = render.as_slice() else {
        anyhow::bail!("Scope requires one render state")
    };
    let flags = c.u32(widget + 0x7c)?;
    ensure!(
        flags & !11 == 0,
        "Unsupported scope bitmap flags {flags:#x}"
    );
    Ok(Layer {
        name: c.string_id(c.u32(widget)?)?,
        mapping: Mapping {
            role: "scope".into(),
            bitmap: Some(bitmap),
            transform: [1., 1., 0., 0.],
            frame,
            address: 0,
            filter: 0,
            functions: Vec::new(),
        },
        uv,
        size,
        origin: [p[0], p[1]],
        offset: [parent[2] + p[2] * parent[4], parent[3] + p[3] * parent[5]],
        scale: [parent[4] * p[4], parent[5] * p[5]],
        flags,
        shader: c.u8(*render)?,
        opacity: 1.,
        color: colors(c, *render, globals)?,
    })
}

pub(crate) fn read(c: &Cache, weapon: &Tag) -> Result<Option<Scope>> {
    let (zoom_levels, magnification) = zoom(c, weapon)?;
    let base = weapon.address()?;
    if zoom_levels == 0 {
        return Ok(None);
    }
    let hud = c
        .reference(base + 0x3c4, Some("chdt"))?
        .context("Scoped weapon has no HUD")?;
    let global = c.only("chgd")?;
    let globals = c
        .block(global.address()?, 0x534)?
        .into_iter()
        .find(|&row| c.u32(row).ok() == Some(0))
        .context("Source Spartan HUD globals are absent")?;
    let mut result = Scope {
        hud,
        zoom_levels,
        magnification,
        layers: Vec::new(),
        excluded: Vec::new(),
    };
    for parent in c.block(result.hud.address()?, 0xdc)? {
        let scoped = state(c, parent, true)?;
        if !scoped && !state(c, parent, false)? {
            continue;
        }
        for widget in c.block(parent + 0x98, 0x9c)? {
            let name = c.string_id(c.u32(widget)?)?;
            let renders = c.block(widget + 0x70, 0x78)?;
            if renders.len() != 1 {
                continue;
            }
            let shader = c.u8(renders[0])?;
            if !scoped && !state(c, widget, true)? && shader != 6 {
                continue;
            }
            let simple = shader <= 1 || shader == 6;
            let mut animated = false;
            let mut opacity = 1.;
            for row in c.block(widget + 0x54, 0x64)? {
                for slot in 0..5 {
                    let Some(animation) = c.reference(row + slot * 20 + 4, Some("chad"))? else {
                        continue;
                    };
                    let at = animation.address()?;
                    // Neutral reticles may vary alpha with firing error. Carry
                    // their zero-error artwork without inventing a native input.
                    let alpha_only = shader == 6
                        && slot == 1
                        && [4, 0x10, 0x1c, 0x28, 0x40, 0x4c, 0x58]
                            .into_iter()
                            .all(|field| c.block(at + field, 32).is_ok_and(|v| v.is_empty()));
                    if alpha_only {
                        let blocks = c.block(at + 0x34, 32)?;
                        ensure!(blocks.len() <= 1, "Reticle alpha animation is ambiguous");
                        if let Some(&block) = blocks.first() {
                            let frames = c.block(block, 8)?;
                            let frame =
                                *frames.first().context("Reticle alpha has no keyframes")?;
                            ensure!(c.f32(frame)? == 0., "Reticle alpha does not start at zero");
                            opacity = c.f32(frame + 4)?;
                            ensure!(
                                (0.0..=1.).contains(&opacity),
                                "Invalid neutral reticle alpha"
                            );
                        }
                    } else {
                        animated = true;
                    }
                }
            }
            if !state(c, widget, false)? || !simple || animated {
                result.excluded.push(json!({"widget":name,"reason":"Dynamic state, meter or animation requires a separate native input mapping"}));
                continue;
            }
            if opacity > 0. {
                let mut layer = layer(c, widget, placement(c, parent)?, globals)?;
                layer.opacity = opacity;
                result.layers.push(layer);
            }
        }
    }
    ensure!(
        result.layers.iter().any(|layer| layer.flags & 8 != 0),
        "Scoped HUD has no supported border mask"
    );
    Ok(Some(result))
}
