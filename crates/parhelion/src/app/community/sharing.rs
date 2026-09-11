use super::*;
use service::{Listing, Submission, TestedWith};

pub(super) struct Form {
    pub recipe: Option<WeaponRecipe>,
    id: String,
    author: String,
    description: String,
    tags: String,
    version: u32,
    sundial: String,
    sunrise: String,
    tested: bool,
    notes: String,
    pub remix_of: String,
    pub permission: bool,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            recipe: None,
            id: String::new(),
            author: String::new(),
            description: String::new(),
            tags: String::new(),
            version: 1,
            sundial: env!("CARGO_PKG_VERSION").into(),
            sunrise: "unknown".into(),
            tested: false,
            notes: "Not yet tested in game.".into(),
            remix_of: String::new(),
            permission: false,
        }
    }
}

impl Window {
    pub(super) fn prepare_share(&mut self, recipe: &WeaponRecipe) {
        let original = self
            .remix_origins
            .get(&recipe.namespace)
            .cloned()
            .unwrap_or_default();
        self.share = Form {
            recipe: Some(recipe.clone()),
            id: recipe
                .namespace
                .trim_start_matches("parhelion.")
                .split(['.', '_', '-'])
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("-"),
            remix_of: original,
            ..Form::default()
        };
        if let Some(entry) = self.catalog.as_ref().and_then(|catalog| {
            catalog
                .recipes
                .iter()
                .find(|entry| entry.namespace == recipe.namespace)
        }) {
            let listing = &entry.listing;
            self.share.id = listing.id.clone();
            self.share.author = listing.author.clone();
            self.share.description = listing.description.clone();
            self.share.tags = listing.tags.join(", ");
            self.share.version = listing.version.saturating_add(1);
            self.share.remix_of = listing.remix_of.clone().unwrap_or_default();
        }
    }

    pub(super) fn sharing(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        current: &WeaponRecipe,
    ) {
        egui::ScrollArea::vertical().id_salt("community-share").show(ui, |ui| {
            ui.heading("Share A Recipe");
            ui.label("Submit a snapshot of your weapon for community review. Your local recipe stays in your library.");
            if ui.add_enabled(self.worker.is_none(), egui::Button::new("Use Current Workbench Recipe")).clicked() {
                self.prepare_share(current);
            }
            if let Some(recipe) = &self.share.recipe { ui.strong(format!("Recipe: {}", recipe.name)); }
            ui.separator();
            ui.add_enabled_ui(self.worker.is_none(), |ui| {
                field(ui, "Creator Credit", &mut self.share.author);
                ui.label("Description");
                ui.add(egui::TextEdit::multiline(&mut self.share.description).desired_rows(3).desired_width(f32::INFINITY));
                field(ui, "Tags", &mut self.share.tags);
                ui.small("Separate tags with commas, for example auto-rifle, solar, experimental.");
                field(ui, "Sundial Version", &mut self.share.sundial);
                field(ui, "Sunrise Version", &mut self.share.sunrise);
                ui.checkbox(&mut self.share.tested, "Tested In Game");
                ui.label("Gameplay Notes And Known Issues");
                ui.add(egui::TextEdit::multiline(&mut self.share.notes).desired_rows(3).desired_width(f32::INFINITY));
                ui.checkbox(&mut self.share.permission, "I Have Permission To Share This Contribution Under GPL-3.0-only");
                ui.small("Include credit and permission for any custom artwork. The submission includes the recipe and the information shown here.");
                let enabled = self.share.recipe.is_some() && self.share.permission;
                if ui.add_enabled(enabled, egui::Button::new("Submit Recipe")).clicked() {
                    match self.share.submission() {
                        Ok(submission) => {
                            self.start(ctx, move |client| client.submit(&submission).map(Outcome::Submitted));
                        }
                        Err(error) => self.message(error, true),
                    }
                }
            });
        });
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

impl Form {
    fn submission(&self) -> Result<Submission, String> {
        let listing = Listing {
            id: self.id.trim().into(),
            author: self.author.trim().into(),
            description: self.description.trim().into(),
            tags: self
                .tags
                .split(',')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect(),
            version: self.version,
            license: "GPL-3.0-only".into(),
            source_url: format!(
                "{}/recipes/{}/recipe.parhelion.json",
                service::ENDPOINT,
                self.id.trim()
            ),
            tested_with: TestedWith {
                sundial: self.sundial.trim().into(),
                sunrise: self.sunrise.trim().into(),
            },
            gameplay_status: if self.tested {
                "author-tested"
            } else {
                "unverified"
            }
            .into(),
            gameplay_notes: self.notes.trim().into(),
            remix_of: (!self.remix_of.trim().is_empty()).then(|| self.remix_of.trim().to_owned()),
        };
        listing.validate()?;
        let recipe = self
            .recipe
            .clone()
            .ok_or("Select a workbench recipe to share")?;
        recipe.validate().map_err(|error| error.to_string())?;
        Ok(Submission { listing, recipe })
    }
}
