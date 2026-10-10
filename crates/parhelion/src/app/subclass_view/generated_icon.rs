//! Generated Icon, beside the Inventory Icon label: the icon drawn as the stock subclass icons
//! are, a diamond in the subclass color, with a symbol from the artwork browser the perk workbench
//! uses at a size of its own. While it is on, the icon card shows it and chooses its symbol, since
//! an icon to borrow or edit would not show.
use super::*;
use crate::subclass::GeneratedIcon;
use crate::subclass::icon::{FULL_SIZE, GLYPH_SIZES};

impl PackageAuthoringApp {
    /// The Generated Icon checkbox. An icon with an image of its own keeps that image, as it does
    /// under a vehicle's silhouette.
    pub(in crate::app) fn draw_generated_icon_toggle(&mut self, ui: &mut egui::Ui) {
        let own_image = self.recipe.overrides.icon_edit.imported_image.is_some();
        let mut on = self.recipe.overrides.subclass_icon.is_some();
        if ui
            .add_enabled(!own_image, egui::Checkbox::new(&mut on, "Generated Icon"))
            .on_hover_text("Draws the icon as a diamond in the subclass color")
            .on_disabled_hover_text("The icon has an image of its own")
            .changed()
        {
            self.recipe.overrides.subclass_icon = on.then(GeneratedIcon::default);
            self.subclass_page.symbol = None;
        }
    }

    /// The generated icon's card in place of the icon's: the icon as `preview` draws it, where
    /// its color comes from, its Symbol and the symbol's size. Returns whether the icon is
    /// generated.
    pub(in crate::app) fn draw_generated_icon_card(
        &mut self,
        ui: &mut egui::Ui,
        preview: Option<&egui::TextureHandle>,
    ) -> bool {
        let overrides = &self.recipe.overrides;
        let Some(current) = overrides
            .subclass_icon
            .clone()
            .filter(|_| overrides.icon_edit.imported_image.is_none())
        else {
            return false;
        };
        let colored = overrides
            .subclass_abilities
            .as_ref()
            .is_some_and(|abilities| abilities.hud_color.is_some());
        let mut glyph = current.glyph.clone();
        // A stock perk's icon arrives once its texture is read.
        match self.subclass_page.symbol.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(icon))) => {
                glyph = Some(icon);
                self.subclass_page.symbol = None;
            }
            Some(Ok(Err(error))) => {
                self.log.push(LogEntry::error(format!("Symbol: {error}")));
                self.subclass_page.symbol = None;
            }
            Some(Err(TryRecvError::Disconnected)) => self.subclass_page.symbol = None,
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        let action = sundial::investment::draw_authoring_item_header(
            ui,
            sundial::investment::AuthoringItemHeader {
                name: "Generated Icon",
                type_name: if colored {
                    "Subclass Color"
                } else {
                    "Super Color"
                },
                hash: None,
                icon: preview,
            },
            "Symbol…",
        )
        .on_hover_text("The artwork at its middle");
        // Clearing the symbol is rare, so it waits in the button's menu.
        action.context_menu(|ui| {
            workbench_style(ui);
            if ui
                .add_enabled(glyph.is_some(), egui::Button::new("Clear Symbol"))
                .clicked()
            {
                glyph = None;
                ui.close();
            }
        });
        let page = &mut self.subclass_page;
        let selection = crate::app::pickers::browser_window(
            ui,
            "subclass-generated-symbol",
            "Choose Symbol",
            &mut page.artwork_query,
            action.clicked(),
            |ui, query, opened, height| {
                page.artwork.draw(
                    ui,
                    query,
                    opened,
                    height,
                    crate::artwork_browser::Browser {
                        packages: Some(self.packages.as_path()),
                        catalog: self.catalog.as_ref(),
                        current: glyph.as_ref(),
                    },
                )
            },
        );
        match selection {
            Some(crate::artwork_browser::Selection::Icon(icon)) => glyph = Some(icon),
            Some(other) => {
                page.symbol =
                    Some(
                        page.artwork
                            .icon(other, &self.packages, self.catalog.as_ref(), ui.ctx()),
                    );
            }
            None => {}
        }
        // A symbol's size, once there is one.
        let mut size = current.size;
        if glyph.is_some() {
            style::tiles(ui, |ui, width| {
                let (changed, reset) = style::stock_tile(
                    ui,
                    (width, "subclass-symbol-size"),
                    ("Symbol Size", "Its size in the diamond"),
                    (size != FULL_SIZE).then_some("100%"),
                    |ui| {
                        let mut value = size;
                        let field = style::tile_field(ui, width, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut value)
                                    .range(GLYPH_SIZES)
                                    .clamp_existing_to_range(false)
                                    .speed(0.5)
                                    .suffix("%"),
                            )
                        });
                        let _ = style::named_control(field, "Symbol Size");
                        (value != size).then_some(value)
                    },
                );
                if let Some(value) = changed {
                    size = value;
                } else if reset {
                    size = FULL_SIZE;
                }
            });
        }
        let next = GeneratedIcon { glyph, size };
        if next != current {
            self.recipe.overrides.subclass_icon = Some(next);
        }
        true
    }
}
