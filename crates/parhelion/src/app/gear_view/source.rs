//! Imported gear uses the ordinary source card without hashing assets on the UI thread.
use super::*;
use crate::icon_edit::ImportedIcon;
use std::sync::{Arc, Mutex};

struct Presentation {
    name: String,
    type_name: String,
    hash: Option<u32>,
    icon: ImportedIcon,
    variants: usize,
    limitations: Vec<String>,
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
    let kind = crate::imported::kind(recipe.kind).ok_or("Unsupported source item kind")?;
    reference
        .validate_reusable(kind)
        .map_err(|e| format!("{e:#}"))?;
    let graph: serde_json::Value = serde_json::from_slice(
        &std::fs::read(reference.directory.join("asset-graph.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let root = reference
        .directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = root
        .join(
            graph["source_icon_png"]
                .as_str()
                .ok_or("Source icon missing")?,
        )
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Source icon escapes its asset folder".into());
    }
    let icon = ImportedIcon::from_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)?;
    Ok(Presentation {
        name: graph["source_name"]
            .as_str()
            .unwrap_or(&recipe.name)
            .to_owned(),
        type_name: graph["source_type"]
            .as_str()
            .unwrap_or(recipe.kind.label())
            .to_owned(),
        hash: graph["source_item"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok()),
        icon,
        variants: graph["gear_art"]["parts"].as_array().map_or(0, Vec::len),
        limitations: graph["limitations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    })
}

pub(super) fn show(app: &mut PackageAuthoringApp, ui: &mut egui::Ui) -> bool {
    let Some(reference) = app.recipe.overrides.imported_graph.as_ref() else {
        return false;
    };
    if crate::imported::kind(app.recipe.kind).is_none() {
        return false;
    }
    let directory = reference.directory.clone();
    let key = format!(
        "{}:{}:{:?}",
        reference.sha256,
        directory.display(),
        app.recipe.kind
    );
    let id = egui::Id::new("imported-gear-source-card");
    let mut card = ui.ctx().data_mut(|data| data.get_temp::<Card>(id));
    if card.as_ref().is_none_or(|card| card.key != key) {
        let loaded = Arc::new(Mutex::new(None));
        let result = loaded.clone();
        let recipe = app.recipe.clone();
        let ctx = ui.ctx().clone();
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
    draw_donor_section_label(
        ui,
        &format!("Source {}", app.recipe.kind.label()),
        Some(
            "Appearance comes from the imported source. Slot, sockets and runtime use a compatible native template.",
        ),
    );
    if let Ok(loaded) = card.loaded.lock() {
        let source = loaded.as_ref().and_then(|value| value.as_ref().ok());
        if card.icon.is_none()
            && let Some(source) = source
        {
            let pixels = source.icon.fit_to(96, 96);
            card.icon = Some(ui.ctx().load_texture(
                "imported-gear-source-icon",
                egui::ColorImage::from_rgba_unmultiplied([96, 96], pixels.as_raw()),
                egui::TextureOptions::LINEAR,
            ));
        }
        let action = sundial::investment::draw_authoring_item_header(
            ui,
            sundial::investment::AuthoringItemHeader {
                name: source.map_or(app.recipe.name.as_str(), |s| s.name.as_str()),
                type_name: source.map_or(app.recipe.kind.label(), |s| s.type_name.as_str()),
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
            Some(Ok(source)) => {
                ui.small(format!("{} source art assignments", source.variants));
                ui.collapsing("Import Details", |ui| {
                    for limitation in &source.limitations {
                        ui.label(limitation);
                    }
                });
            }
        }
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, card));
    true
}
