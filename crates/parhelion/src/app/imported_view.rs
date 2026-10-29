//! Imported source cards and override labels, read without blocking the UI thread.
use super::*;
use crate::icon_edit::ImportedIcon;
use std::sync::{Arc, Mutex};

struct Presentation {
    name: Option<String>,
    /// The source's item type, else the recipe's own. `None` keeps the base weapon's.
    type_name: Option<String>,
    hash: Option<u32>,
    icon: Option<ImportedIcon>,
    dyes: bool,
    animations: bool,
}

impl Presentation {
    fn name<'a>(&'a self, recipe: &'a WeaponRecipe) -> &'a str {
        self.name.as_deref().unwrap_or(&recipe.name)
    }
}

type Read = Result<Presentation, String>;

#[derive(Clone)]
struct Card {
    key: String,
    loaded: Arc<Mutex<Option<Read>>>,
    icon: Option<egui::TextureHandle>,
}

fn read(recipe: &WeaponRecipe) -> Read {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("Source assets missing")?;
    let graph: serde_json::Value = serde_json::from_slice(
        &std::fs::read(reference.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let validate = || {
        if recipe.kind.is_weapon() {
            if graph["kind"].as_str().is_some_and(|kind| kind != "weapon") {
                return Err("The selected assets are not a weapon appearance".to_owned());
            }
            let item = graph["item_hash"]
                .as_u64()
                .and_then(|hash| u32::try_from(hash).ok())
                .ok_or("Imported weapon identity missing")?;
            reference
                .validate_opened(item)
                .map_err(|e| format!("{e:#}"))
        } else {
            let kind = crate::imported::kind(recipe.kind).ok_or("Unsupported source item kind")?;
            reference
                .validate_reusable_opened(kind)
                .map_err(|e| format!("{e:#}"))
        }
    };
    validate()?;
    let root = reference
        .directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let icon = graph["source_icon_png"]
        .as_str()
        .map(|file| {
            let path = root.join(file).canonicalize().map_err(|e| e.to_string())?;
            if !path.starts_with(&root) {
                return Err("Source icon escapes its asset folder".into());
            }
            ImportedIcon::from_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)
        })
        .transpose()?;
    validate()?;
    Ok(Presentation {
        name: graph["source_name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned),
        type_name: graph["source_type"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| recipe.type_name.clone()),
        hash: graph["source_item"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok()),
        icon,
        // Emission replaces all base dye rows when the graph declares its own dyes.
        dyes: graph["dyes"].is_array(),
        // Only linked first-person payloads replace the native animation selection.
        animations: graph["animation"]["first_person_status"] == "linked",
    })
}

fn card(recipe: &WeaponRecipe, ctx: &egui::Context) -> Option<Card> {
    let reference = recipe.overrides.imported_graph.as_ref()?;
    if !recipe.kind.is_weapon() && crate::imported::kind(recipe.kind).is_none() {
        return None;
    }
    let directory = reference.directory.clone();
    let key = format!(
        "{}:{}:{:?}",
        reference.sha256,
        directory.display(),
        recipe.kind
    );
    let id = egui::Id::new("imported-source-card");
    let mut card = ctx.data_mut(|data| data.get_temp::<Card>(id));
    if card.as_ref().is_none_or(|card| card.key != key) {
        let loaded = Arc::new(Mutex::new(None));
        let result = loaded.clone();
        let recipe = recipe.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let value = read(&recipe);
            if let Ok(mut slot) = result.lock() {
                *slot = Some(value);
            }
            ctx.request_repaint();
        });
        card = Some(Card {
            key,
            loaded,
            icon: None,
        });
    }
    let mut card = card.unwrap();
    if let Ok(loaded) = card.loaded.lock()
        && card.icon.is_none()
        && let Some(icon) = loaded
            .as_ref()
            .and_then(|value| value.as_ref().ok())
            .and_then(|source| source.icon.as_ref())
    {
        let pixels = icon.fit_to(96, 96);
        card.icon = Some(ctx.load_texture(
            "imported-source-icon",
            egui::ColorImage::from_rgba_unmultiplied([96, 96], pixels.as_raw()),
            egui::TextureOptions::LINEAR,
        ));
    }
    ctx.data_mut(|data| data.insert_temp(id, card.clone()));
    Some(card)
}

pub(super) fn show(app: &mut PackageAuthoringApp, ui: &mut egui::Ui) -> bool {
    show_as(app, ui, &format!("Source {}", app.recipe.kind.label()))
}

pub(super) fn show_as(app: &mut PackageAuthoringApp, ui: &mut egui::Ui, heading: &str) -> bool {
    let Some(card) = card(&app.recipe, ui.ctx()) else {
        return false;
    };
    let directory = app
        .recipe
        .overrides
        .imported_graph
        .as_ref()
        .unwrap()
        .directory
        .clone();
    draw_donor_section_label(
        ui,
        heading,
        Some(
            "The imported source supplies the model and materials. Other parts follow the recipe's overrides and native base.",
        ),
    );
    // A weapon import without its own type is the base weapon's type.
    let base_type = app
        .recipe
        .donor
        .item_hash
        .parse_u32()
        .ok()
        .filter(|_| app.recipe.kind.is_weapon())
        .and_then(|hash| app.donor_summaries.iter().find(|donor| donor.hash == hash))
        .map(|donor| donor.type_name.as_str());
    if let Ok(loaded) = card.loaded.lock() {
        let source = loaded.as_ref().and_then(|value| value.as_ref().ok());
        let action = sundial::investment::draw_authoring_item_header(
            ui,
            sundial::investment::AuthoringItemHeader {
                name: source.map_or(app.recipe.name.as_str(), |s| s.name(&app.recipe)),
                type_name: source
                    .and_then(|s| s.type_name.as_deref())
                    .or(base_type)
                    .unwrap_or(app.recipe.kind.label()),
                hash: source.and_then(|s| s.hash),
                icon: card.icon.as_ref(),
            },
            "Open Imported Assets",
        );
        if action.clicked()
            && let Err(error) = sundial::package_authoring::open_directory(&directory)
        {
            app.log.push(LogEntry::error(error));
        }
        match loaded.as_ref() {
            None => {
                ui.spinner();
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            Some(Ok(_)) => {}
        }
    }
    true
}

/// The choice an attached ornament binds to: its socket, its choice and the stock plug it is laid
/// out on.
type Choice = (usize, usize, u32);

/// An attached ornament as read: its choice, its icon and its rarity.
type Attached = (Choice, egui::ColorImage, Option<u8>);

#[derive(Clone)]
struct Ornaments {
    key: String,
    loaded: Arc<Mutex<Option<Vec<Attached>>>>,
    shown: Option<Vec<(Choice, egui::TextureHandle, Option<u8>)>>,
}

/// What an imported ornament shows in place of the stock plug it is laid out on.
pub(super) struct Ornament {
    pub(super) icon: sundial::investment::IconOverride,
    /// The tier as packages store it, when the import names one.
    pub(super) rarity: Option<u8>,
}

/// The choice an attached graph binds to as an ornament, and the icon and rarity the build gives
/// its private plug.
fn read_ornament(attachment: &parhelion_import::GraphReference) -> Option<Attached> {
    let graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(attachment.directory.join("asset-graph.json")).ok()?)
            .ok()?;
    let link = graph.get("ornament")?;
    let number = |key: &str| link[key].as_u64();
    let choice = (
        usize::try_from(number("target_socket")?).ok()?,
        usize::try_from(number("choice_index")?).ok()?,
        u32::try_from(number("native_plug")?).ok()?,
    );
    let root = attachment.directory.canonicalize().ok()?;
    let path = root
        .join(graph["ornament_icon_png"].as_str()?)
        .canonicalize()
        .ok()?;
    if !path.starts_with(&root) {
        return None;
    }
    let pixels = ImportedIcon::from_bytes(&std::fs::read(path).ok()?)
        .ok()?
        .fit_to(96, 96);
    // The plug keeps its layout plug's rarity when the link names none.
    let rarity = serde_json::from_value::<crate::recipe::RecipeRarity>(link["rarity"].clone())
        .ok()
        .map(|rarity| rarity as u8 + 1);
    Some((
        choice,
        egui::ColorImage::from_rgba_unmultiplied([96, 96], pixels.as_raw()),
        rarity,
    ))
}

/// The imported ornament that takes `plug`'s place as `choice` of `socket`. The choice shows the
/// stock ornament it is laid out on otherwise, while the build gives its private plug this icon
/// and rarity. The icon is pending while the recipe's attachments are read, so a choice never
/// shows the stock icon first.
pub(super) fn ornament(
    recipe: &WeaponRecipe,
    ctx: &egui::Context,
    (socket, choice, plug): Choice,
) -> Option<Ornament> {
    let attachments = &recipe.overrides.imported_graph.as_ref()?.attachments;
    if attachments.is_empty() {
        return None;
    }
    let key = attachments
        .iter()
        .map(|graph| format!("{}:{}", graph.sha256, graph.directory.display()))
        .collect::<Vec<_>>()
        .join("|");
    let id = egui::Id::new("imported-ornament-icons");
    let mut ornaments = ctx
        .data_mut(|data| data.get_temp::<Ornaments>(id))
        .filter(|ornaments| ornaments.key == key)
        .unwrap_or_else(|| {
            let loaded = Arc::new(Mutex::new(None));
            let result = loaded.clone();
            let attachments = attachments.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let value = attachments.iter().filter_map(read_ornament).collect();
                if let Ok(mut slot) = result.lock() {
                    *slot = Some(value);
                }
                ctx.request_repaint();
            });
            Ornaments {
                key,
                loaded,
                shown: None,
            }
        });
    if ornaments.shown.is_none()
        && let Some(read) = ornaments
            .loaded
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    {
        ornaments.shown = Some(
            read.into_iter()
                .map(|(choice, image, rarity)| {
                    let name = format!("imported-ornament-{}-{}", choice.0, choice.1);
                    let icon = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                    (choice, icon, rarity)
                })
                .collect(),
        );
    }
    let ornament = match &ornaments.shown {
        None => Some(Ornament {
            icon: sundial::investment::IconOverride::Pending,
            rarity: None,
        }),
        Some(shown) => shown
            .iter()
            .find(|(at, ..)| *at == (socket, choice, plug))
            .map(|(_, icon, rarity)| Ornament {
                icon: sundial::investment::IconOverride::Texture(icon.id()),
                rarity: *rarity,
            }),
    };
    ctx.data_mut(|data| data.insert_temp(id, ornaments));
    ornament
}

pub(super) enum Part {
    Model,
    Colors,
    Animations,
}

/// Returns false only when this part still comes from the native recipe selection.
pub(super) fn part(recipe: &WeaponRecipe, ui: &mut egui::Ui, part: Part) -> bool {
    let Some(card) = card(recipe, ui.ctx()) else {
        return false;
    };
    let Ok(loaded) = card.loaded.lock() else {
        return true;
    };
    let source = loaded.as_ref().and_then(|value| value.as_ref().ok());
    if source.is_some_and(|source| match part {
        Part::Model => false,
        Part::Colors => false,
        Part::Animations => !source.animations,
    }) {
        return false;
    }
    let native_colors = matches!(part, Part::Colors) && source.is_some_and(|source| !source.dyes);
    let label = match part {
        Part::Model => "Model",
        Part::Colors if native_colors => "Materials",
        Part::Colors => "Colors & Materials",
        Part::Animations => "Animations",
    };
    ui.horizontal_wrapped(|ui| {
        if matches!(part, Part::Colors) {
            ui.strong(label);
        } else {
            let width = PackageAuthoringApp::appearance_label_width(ui);
            ui.add_sized(
                [width, ui.spacing().interact_size.y],
                egui::Label::new(egui::RichText::new(label).weak()),
            );
        }
        match loaded.as_ref() {
            None => {
                ui.spinner();
                ui.label("Reading Imported Source…");
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            Some(Ok(source)) => {
                ui.label(source.name(recipe))
                    .on_hover_text("Supplied by the imported assets.");
            }
        }
    });
    !native_colors
}
