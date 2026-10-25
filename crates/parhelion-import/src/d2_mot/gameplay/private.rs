//! Source plugs that Shadowkeep has no counterpart for become private plugs on the socket where
//! the base keeps its own plug, when their controllers translate. The private plug carries the
//! source name, description and icon, so the socket shows the source perk and runs its
//! translated behavior instead of the base's.
use super::perks::{self, lifecycle, presentation};
use crate::d2_mot::reader::{Reader, write_json};
use anyhow::{Context, Result};
use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;

/// The edge of the square artwork a recipe embeds.
const ICON_EDGE: u32 = 96;

/// Intrinsic plugs (`67FBA961`) set the weapon's frame, which belongs to the base's runtime.
const INTRINSIC_CATEGORY: u64 = 0x67FB_A961;
/// Weapon perk plugs, as opposed to shaders (14), masterworks and trackers (13) and mods (37).
const PERK_ITEM_KIND: u64 = 15;

/// Add a private plug for each socket fallback in `report` whose source plug translates. A perk
/// socket the source fills with nothing it can run is disabled, so the base's own perk does not
/// stand in for the source's. Return what was translated, disabled and kept.
pub fn apply(
    modern: &Path,
    native: &Path,
    report: &Value,
    output: &Path,
    recipe: &mut Value,
) -> Result<Value> {
    let fallbacks = report["fallbacks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                usize::try_from(row["socket"].as_u64()?).ok()?,
                u32::try_from(row["source_default"].as_u64()?).ok()?,
                u32::try_from(row["retained_default"].as_u64()?).ok()?,
            ))
        })
        .collect::<Vec<_>>();
    let mut translated = Vec::new();
    let mut retained = Vec::new();
    let mut readers = None;
    for (socket, plug, base_plug) in fallbacks {
        if readers.is_none() {
            readers = Some((
                Reader::discovery(modern, &output.join("source"), true)?,
                Reader::discovery(native, &output.join("native"), false)?,
            ));
        }
        let Some((source, native_reader)) = readers.as_mut() else {
            continue;
        };
        let authored = recipe["overrides"]["socket_plug_variants"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["socket_index"] == socket));
        if authored {
            retained.push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),"reason":"the socket already has an authored private plug"}));
            continue;
        }
        match private_plug(source, native_reader, socket, plug, base_plug) {
            Ok((variant, omitted)) => {
                pin_column(recipe, socket, base_plug)?;
                let name = variant["name"].clone();
                let variants = &mut recipe["overrides"]["socket_plug_variants"];
                if !variants.is_array() {
                    *variants = json!([]);
                }
                variants
                    .as_array_mut()
                    .context("socket plug variants")?
                    .push(variant);
                translated.push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),
                    "name":name,"template_plug":format!("{base_plug:08X}"),"omitted":omitted,
                    "gameplay_verified":false}));
            }
            Err(error) => retained
                .push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),
                "base_plug":format!("{base_plug:08X}"),"reason":format!("{error:#}")})),
        }
    }
    let disabled = disable_unfilled_perks(report, recipe)?;
    let result = json!({"translated":translated,"retained":retained,"disabled":disabled});
    std::fs::create_dir_all(output)?;
    write_json(&output.join("private-perks.json"), &result)?;
    Ok(result)
}

/// Disable each non-intrinsic perk socket that carries no source plug, translated or native.
fn disable_unfilled_perks(report: &Value, recipe: &mut Value) -> Result<Vec<Value>> {
    let mut disabled = Vec::new();
    for (index, socket) in report["native_sockets"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(default) = socket["choices"]
            .as_array()
            .and_then(|choices| choices.iter().find(|choice| choice["default"] == true))
        else {
            continue;
        };
        if socket["socket_type"] == u16::MAX
            || default["item_kind"].as_u64() != Some(PERK_ITEM_KIND)
            || default["category"].as_u64() == Some(INTRINSIC_CATEGORY)
        {
            continue;
        }
        let private = recipe["overrides"]["socket_plug_variants"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["socket_index"] == index));
        let column = &recipe["overrides"]["socket_columns"][index];
        let filled = column["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice != &json!(format!("0x{:08X}", default["hash"].as_u64().unwrap_or(0)))
            })
        });
        let source_default = report["mapped"]["socket_columns"][index]["choices"]
            .as_array()
            .is_some_and(|choices| !choices.is_empty());
        let fallback = report["fallbacks"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["socket"] == index && row.get("retained_default").is_some())
        });
        if private || (source_default && !fallback) || filled {
            continue;
        }
        let columns = &mut recipe["overrides"]["socket_columns"];
        if !columns.is_array() {
            *columns = json!([]);
        }
        let columns = columns.as_array_mut().context("socket columns")?;
        if columns.len() <= index {
            columns.resize(index + 1, Value::Null);
        }
        columns[index] = json!({"choices":[],"socket_type":u16::MAX});
        disabled.push(json!({"socket":index,"base_plug":format!("{:08X}", default["hash"].as_u64().unwrap_or(0))}));
    }
    Ok(disabled)
}

fn private_plug(
    source: &mut Reader,
    native: &mut Reader,
    socket: usize,
    plug: u32,
    base_plug: u32,
) -> Result<(Value, Vec<Value>)> {
    let assigned = perks::assigned(source, plug)?;
    let presentation = presentation::read(source, plug)?;
    let translation = lifecycle::translate(&assigned, &presentation.name);
    anyhow::ensure!(
        !translation.programs.is_empty(),
        "no source controller translates: {}",
        serde_json::to_string(&translation.omitted)?
    );
    // The private plug clones the base's plug for its item layout. Each program replaces the
    // runtime of one of that plug's finished perks, a different one for each program.
    let tag = crate::tiger::shadowkeep::item_tag(native, base_plug)?;
    let item = native.tag(tag, Some(0x8080_7BEA))?;
    let mut templates = sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
        .map_err(anyhow::Error::msg)?;
    templates.dedup();
    anyhow::ensure!(
        !templates.is_empty(),
        "the base plug has no finished perk to carry the translated programs"
    );
    let mut omitted = translation.omitted;
    let mut sandbox_perks = Vec::new();
    let mut projectile = None;
    for (index, program) in translation.programs.iter().enumerate() {
        let Some(template) = templates.get(index) else {
            omitted.push(
                json!({"program":program.name,"trigger":serde_json::to_value(program.trigger)?,
                "reason":"the base plug has no further finished perk to carry this program"}),
            );
            continue;
        };
        projectile = projectile.or_else(|| program.projectile_dependency());
        sandbox_perks.push(json!({"source_perk_index":template,
            "program":serde_json::to_value(program)?,"runtime_values":[],"action_float_values":[]}));
    }
    let mut variant = json!({"socket_index":socket,"choice_index":0,
        "source_plug_hash":format!("0x{base_plug:08X}"),"name":presentation.name,
        "description":presentation.description,"replace_effects":true,
        "sandbox_perks":sandbox_perks});
    if let Some(dependency) = projectile {
        variant["projectile"] = serde_json::to_value(dependency)?;
    }
    if let Some(icon) = presentation.icon_png.as_deref().and_then(icon) {
        variant["icon"] =
            json!({"source":"image","name":presentation.name,"image":{"png_base64":icon}});
    }
    Ok((variant, omitted))
}

/// A square source icon at the recipe's artwork size, as base64 PNG.
fn icon(png: &[u8]) -> Option<String> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    if image.width() != image.height() || image.width() == 0 {
        return None;
    }
    let image = if image.width() == ICON_EDGE {
        image
    } else {
        image::imageops::resize(
            &image,
            ICON_EDGE,
            ICON_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}

/// Keep the base's plug as the socket's choice, so the private plug replaces exactly it.
fn pin_column(recipe: &mut Value, socket: usize, base_plug: u32) -> Result<()> {
    let columns = &mut recipe["overrides"]["socket_columns"];
    if !columns.is_array() {
        *columns = json!([]);
    }
    let columns = columns.as_array_mut().context("socket columns")?;
    if columns.len() <= socket {
        columns.resize(socket + 1, Value::Null);
    }
    if columns[socket].is_null() {
        columns[socket] = json!({"choices":[format!("0x{base_plug:08X}")]});
    }
    Ok(())
}
