use super::*;
use std::collections::BTreeSet;

pub(super) struct AuthoredTemplate {
    pub(super) weapon: String,
    pub(super) variant: WeaponSocketPlugVariantRecipe,
}

impl Workbench {
    pub(super) fn load_authored_templates(
        &mut self,
        library: Option<&RecipeLibrary>,
        draft: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        self.initialize();
        let (mut templates, mut warnings) = load_saved(library);
        if let Some(perks) = &self.library {
            let recipes = templates.iter().filter_map(|entry| {
                let hash = entry.variant.source_plug_hash.parse_u32().unwrap_or_default();
                if !entry.variant.replace_effects && catalog.item_definition_tag(hash).is_none() {
                    warnings.push(format!("Could not import a custom perk from {} because its stock template {hash:08X} is unavailable. The weapon recipe is unchanged.", entry.weapon));
                    None
                } else {
                    Some(from_variant(&entry.variant, catalog))
                }
            }).collect::<Vec<_>>();
            match perks.import_embedded(recipes) {
                Ok(report) => {
                    warnings.extend(report.errors);
                    if report.added > 0 {
                        self.message = Some(format!(
                            "Added {} custom perks from saved weapon recipes to the library.",
                            report.added
                        ));
                    }
                }
                Err(error) => warnings.push(error),
            }
            warnings.extend(self.scan_library());
        }
        append_templates(&mut templates, draft.clone());
        self.authored_templates = Some(templates);
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
    }

    pub(super) fn draw_templates(&mut self, ui: &mut egui::Ui, catalog: &InvestmentCatalog) {
        let stock = self.templates.get_or_insert_with(|| {
            catalog.perk_template_choices_from(crate::package_profile::is_stock_item_definition)
        });
        // The stock rows read a description and an effect list per perk, so they are built
        // once per catalog. The authored rows are few and follow the library, so they are
        // read each frame.
        let stock_rows = self.template_rows.get_or_insert_with(|| {
            stock
                .iter()
                .enumerate()
                .map(|(index, choice)| Row::stock(index, choice, catalog))
                .collect()
        });
        let authored = self.authored_templates.as_deref().unwrap_or_default();
        let authored_rows = authored
            .iter()
            .enumerate()
            .map(|(index, entry)| Row::authored(index, entry, catalog))
            .collect::<Vec<_>>();
        // The type list is read off the rows, so it offers only types some perk has.
        let types = authored_rows
            .iter()
            .chain(stock_rows.iter())
            .map(|row| row.type_name.as_str())
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let picked = pickers::popup(
            ui,
            "perk-template",
            "Copy Existing…",
            &mut self.template_query,
            |ui, query, reset, height| {
                let mut filters = Filters::load(ui);
                let (changed, toolbar_height) = filters.toolbar(ui, &types);
                filters.store(ui);
                let mut rows = authored_rows
                    .iter()
                    .chain(stock_rows.iter())
                    .filter(|row| filters.keeps(row) && pickers::matches(query, &row.search))
                    .collect::<Vec<_>>();
                rows.sort_by(|a, b| sort_key(filters.order, a).cmp(&sort_key(filters.order, b)));
                pickers::results(
                    ui,
                    "perk-template-results",
                    rows.len(),
                    (height - toolbar_height - ui.spacing().item_spacing.y).max(90.0),
                    reset || changed,
                    sundial::investment::authoring_choice_row_height(ui),
                    |ui, index| {
                        let row = rows[index];
                        catalog
                            .draw_authoring_choice_row(
                                ui,
                                Some(row.hash),
                                &row.name,
                                Some(&row.detail),
                                false,
                            )
                            .clicked()
                            .then_some((row.authored, row.index))
                    },
                )
            },
        );
        let Some((is_authored, index)) = picked else {
            return;
        };
        let recipe = if is_authored {
            from_variant(&authored[index].variant, catalog)
        } else {
            let choice = &stock[index];
            let mut recipe = PerkRecipe::new();
            recipe.name = format!("Custom {}", choice.representative_name);
            recipe.template_plug = choice.representative_hash.into();
            recipe.description = catalog
                .perk_description(choice.representative_hash)
                .unwrap_or_default()
                .to_owned();
            recipe.effects = catalog
                .item_sandbox_perk_indices(choice.representative_hash)
                .into_iter()
                .map(PerkRecipe::effect)
                .collect();
            recipe.stats = catalog
                .item_stat_contributions(choice.representative_hash)
                .into_iter()
                .map(|stat| WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value: stat.value,
                })
                .collect();
            recipe
        };
        self.add_document(Document::new(recipe, None));
        self.page = Page::Effects;
    }
}

/// One row of the Copy Existing picker, with the facts its filters and orders read: the
/// perk's type as the game labels it, whether it carries an effect, and whether the game
/// shipped it or a saved weapon carries it.
pub(super) struct Row {
    pub(super) authored: bool,
    pub(super) index: usize,
    pub(super) hash: u32,
    pub(super) name: String,
    pub(super) detail: String,
    pub(super) type_name: String,
    pub(super) effects: bool,
    /// The lowercase name and type, so ordering allocates nothing per frame.
    key: String,
    type_key: String,
    /// What the search box matches against: the name, the detail line and the hash.
    search: String,
}

impl Row {
    pub(super) fn new(
        authored: bool,
        index: usize,
        hash: u32,
        name: String,
        detail: String,
        type_name: String,
        effects: bool,
    ) -> Self {
        Self {
            key: name.to_lowercase(),
            type_key: type_name.to_lowercase(),
            search: format!("{name} {detail} {hash:08X}"),
            authored,
            index,
            hash,
            name,
            detail,
            type_name,
            effects,
        }
    }

    fn stock(index: usize, choice: &WeaponSandboxPerkChoice, catalog: &InvestmentCatalog) -> Self {
        let hash = choice.representative_hash;
        let detail = format!(
            "{} · {}",
            choice.representative_type_name,
            catalog.perk_description(hash).unwrap_or_default()
        );
        Self::new(
            false,
            index,
            hash,
            choice.representative_name.clone(),
            detail,
            choice.representative_type_name.clone(),
            !catalog.item_sandbox_perk_indices(hash).is_empty(),
        )
    }

    fn authored(index: usize, entry: &AuthoredTemplate, catalog: &InvestmentCatalog) -> Self {
        let variant = &entry.variant;
        let hash = variant.source_plug_hash.parse_u32().unwrap_or_default();
        let name = variant
            .name
            .clone()
            .unwrap_or_else(|| catalog.plug_label(hash, false));
        let detail = format!(
            "{} · {}",
            entry.weapon,
            variant.description.as_deref().unwrap_or_default()
        );
        // The type reads off the classification donor when the perk names one, which is
        // the type the Basics row shows for it.
        let type_name = catalog
            .item_type_name(
                variant
                    .classification_donor_hash
                    .as_ref()
                    .unwrap_or(&variant.source_plug_hash)
                    .parse_u32()
                    .unwrap_or_default(),
            )
            .unwrap_or_default();
        let effects = !variant.sandbox_perks.is_empty()
            || !variant.additional_sandbox_perks.is_empty()
            || (!variant.replace_effects && !catalog.item_sandbox_perk_indices(hash).is_empty());
        Self::new(true, index, hash, name, detail, type_name, effects)
    }
}

/// Whether the game shipped the perk or a saved weapon carries it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Origin {
    #[default]
    Any,
    Stock,
    Custom,
}

impl Origin {
    const ALL: [Self; 3] = [Self::Any, Self::Stock, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::Any => "Stock or Custom",
            Self::Stock => "Stock Perks",
            Self::Custom => "Custom Perks",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Any => "Every perk, whether the game shipped it or a saved weapon carries it.",
            Self::Stock => "Perks the game ships.",
            Self::Custom => {
                "Perks saved with your weapons, and the ones on the weapon being edited."
            }
        }
    }
}

/// Whether the perk carries at least one effect, or is a name, description and icon alone.
/// Most stock plugs are the latter (emotes, ornaments, shaders, stat allocations), so the
/// picker opens on the ones with effects.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Effects {
    Any,
    #[default]
    With,
    Without,
}

impl Effects {
    const ALL: [Self; 3] = [Self::Any, Self::With, Self::Without];

    fn label(self) -> &'static str {
        match self {
            Self::Any => "Effects or None",
            Self::With => "With Effects",
            Self::Without => "Without Effects",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Any => "Every perk, whether or not it carries an effect.",
            Self::With => "Perks that carry at least one effect.",
            Self::Without => "Perks with no effect: a name, description and icon alone.",
        }
    }
}

/// How the results are ordered. Order changes presentation only; it never hides a perk.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Order {
    /// Saved custom perks first, then by name: the order the picker always had.
    #[default]
    CustomFirst,
    Name,
    Type,
}

impl Order {
    const ALL: [Self; 3] = [Self::CustomFirst, Self::Name, Self::Type];

    fn label(self) -> &'static str {
        match self {
            Self::CustomFirst => "Custom First",
            Self::Name => "Name",
            Self::Type => "Type",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::CustomFirst => "Saved custom perks first, then by name.",
            Self::Name => "By name alone.",
            Self::Type => "By perk type, then by name.",
        }
    }
}

/// The comparable key for one row. Every order ends with the name and hash, so results
/// never shuffle between frames.
pub(super) fn sort_key(order: Order, row: &Row) -> (bool, &str, &str, u32) {
    match order {
        Order::CustomFirst => (!row.authored, "", &row.key, row.hash),
        Order::Name => (false, "", &row.key, row.hash),
        // Perks the game gives no type label sort after every labelled one.
        Order::Type => (row.type_key.is_empty(), &row.type_key, &row.key, row.hash),
    }
}

/// The picker's filters and order, kept in egui memory so they outlive the popup.
#[derive(Clone, Default, PartialEq)]
pub(super) struct Filters {
    pub(super) origin: Origin,
    pub(super) effects: Effects,
    pub(super) order: Order,
    /// The perk type to show, as the game labels it; empty shows every type.
    pub(super) type_name: String,
}

impl Filters {
    // The toolbar wraps, which gives each control a child `Ui`; a stable id keeps the
    // choice readable from the same place every frame.
    fn id() -> egui::Id {
        egui::Id::new("perk-template-filters")
    }

    fn load(ui: &egui::Ui) -> Self {
        ui.data(|data| data.get_temp::<Self>(Self::id()).unwrap_or_default())
    }

    fn store(&self, ui: &egui::Ui) {
        ui.data_mut(|data| data.insert_temp(Self::id(), self.clone()));
    }

    pub(super) fn keeps(&self, row: &Row) -> bool {
        (match self.origin {
            Origin::Any => true,
            Origin::Stock => !row.authored,
            Origin::Custom => row.authored,
        }) && (match self.effects {
            Effects::Any => true,
            Effects::With => row.effects,
            Effects::Without => !row.effects,
        }) && (self.type_name.is_empty() || row.type_name == self.type_name)
    }

    /// Draws the filter and sort controls. Returns whether a choice changed, and the height
    /// the toolbar took, which the result list gives up.
    fn toolbar(&mut self, ui: &mut egui::Ui, types: &[&str]) -> (bool, f32) {
        let before = self.clone();
        // A type no perk has any more reads as every type rather than as an empty list.
        if !self.type_name.is_empty() && !types.contains(&self.type_name.as_str()) {
            self.type_name.clear();
        }
        let response = ui.horizontal_wrapped(|ui| {
            // Four combos do not fit one line in a narrow window. Each is held to one width
            // and truncates to it, with its full reading on hover, and one that would run
            // past the edge starts the next line instead.
            const FILTER_WIDTH: f32 = 150.0;
            let fit = |ui: &mut egui::Ui| {
                if ui.available_rect_before_wrap().width() < FILTER_WIDTH + ui.spacing().item_spacing.x {
                    ui.end_row();
                }
            };
            let type_text = if self.type_name.is_empty() {
                "All Types".to_owned()
            } else {
                self.type_name.clone()
            };
            controls::sized(ui, FILTER_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("perk-template-type")
                    .width(FILTER_WIDTH)
                    .truncate()
                    .selected_text(&type_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.type_name, String::new(), "All Types");
                        for &name in types {
                            ui.selectable_value(&mut self.type_name, name.to_owned(), name);
                        }
                    })
                    .response
                    .on_hover_text(format!("{type_text}\nShow one type of perk, as the game labels it."));
                pickers::name_combo(ui, "perk-template-type", "Perk Type");
            });
            fit(ui);
            let origin_text = self.origin.label();
            controls::sized(ui, FILTER_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("perk-template-origin")
                    .width(FILTER_WIDTH)
                    .truncate()
                    .selected_text(origin_text)
                    .show_ui(ui, |ui| {
                        for choice in Origin::ALL {
                            ui.selectable_value(&mut self.origin, choice, choice.label())
                                .on_hover_text(choice.hint());
                        }
                    })
                    .response
                    .on_hover_text(format!("{origin_text}\nFilter by whether the game shipped the perk or a saved weapon carries it."));
                pickers::name_combo(ui, "perk-template-origin", "Origin");
            });
            fit(ui);
            let effects_text = self.effects.label();
            controls::sized(ui, FILTER_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("perk-template-effects")
                    .width(FILTER_WIDTH)
                    .truncate()
                    .selected_text(effects_text)
                    .show_ui(ui, |ui| {
                        for choice in Effects::ALL {
                            ui.selectable_value(&mut self.effects, choice, choice.label())
                                .on_hover_text(choice.hint());
                        }
                    })
                    .response
                    .on_hover_text(format!("{effects_text}\nFilter by whether the perk carries an effect."));
                pickers::name_combo(ui, "perk-template-effects", "Effects");
            });
            fit(ui);
            let order_text = format!("Sort: {}", self.order.label());
            controls::sized(ui, FILTER_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("perk-template-order")
                    .width(FILTER_WIDTH)
                    .truncate()
                    .selected_text(order_text.clone())
                    .show_ui(ui, |ui| {
                        for choice in Order::ALL {
                            ui.selectable_value(&mut self.order, choice, choice.label())
                                .on_hover_text(choice.hint());
                        }
                    })
                    .response
                    .on_hover_text(format!("{order_text}\nOrder the results. Sorting never hides a perk."));
                pickers::name_combo(ui, "perk-template-order", "Sort Order");
            });
        });
        (*self != before, response.response.rect.height())
    }
}

pub(super) fn from_variant(
    variant: &WeaponSocketPlugVariantRecipe,
    catalog: &InvestmentCatalog,
) -> PerkRecipe {
    let mut recipe = PerkRecipe::new();
    let hash = variant.source_plug_hash.parse_u32().unwrap_or_default();
    recipe.template_plug = variant.source_plug_hash.clone();
    recipe.icon = variant.icon.clone();
    recipe.name = variant
        .name
        .clone()
        .unwrap_or_else(|| catalog.plug_label(hash, false));
    recipe.description = variant.description.clone().unwrap_or_else(|| {
        catalog
            .perk_description(hash)
            .unwrap_or_default()
            .to_owned()
    });
    recipe.classification = variant.classification_donor_hash.clone();
    if !variant.replace_effects {
        recipe.effects = catalog
            .item_sandbox_perk_indices(hash)
            .into_iter()
            .map(PerkRecipe::effect)
            .collect();
        recipe.stats = catalog
            .item_stat_contributions(hash)
            .into_iter()
            .map(|stat| WeaponStatOverride {
                definition_index: stat.definition_index,
                value: stat.value,
            })
            .collect();
    }
    for &index in &variant.additional_sandbox_perks {
        if !recipe
            .effects
            .iter()
            .any(|effect| effect.source_perk_index == index)
        {
            recipe.effects.push(PerkRecipe::effect(index));
        }
    }
    for effect in &variant.sandbox_perks {
        if let Some(existing) = recipe
            .effects
            .iter_mut()
            .find(|existing| existing.source_perk_index == effect.source_perk_index)
        {
            *existing = effect.clone();
        } else {
            recipe.effects.push(effect.clone());
        }
    }
    for stat in &variant.investment_stats {
        if let Some(existing) = recipe
            .stats
            .iter_mut()
            .find(|existing| existing.definition_index == stat.definition_index)
        {
            *existing = stat.clone();
        } else {
            recipe.stats.push(stat.clone());
        }
    }
    recipe
}

/// Source loading returns its own diagnostics so callers can display them in context.
pub(super) fn load(
    library: Option<&RecipeLibrary>,
    draft: &WeaponRecipe,
) -> (Vec<AuthoredTemplate>, Vec<String>) {
    let (mut templates, warnings) = load_saved(library);
    append_templates(&mut templates, draft.clone());
    (templates, warnings)
}

fn load_saved(library: Option<&RecipeLibrary>) -> (Vec<AuthoredTemplate>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut recipes = Vec::new();
    if let Some(library) = library {
        match library.scan() {
            Ok(scan) => {
                warnings.extend(scan.errors);
                for entry in scan.entries {
                    match WeaponRecipe::load_json(&entry.path) {
                        Ok(recipe) => recipes.push(recipe),
                        Err(error) => warnings.push(error.to_string()),
                    }
                }
            }
            Err(error) => warnings.push(error),
        }
    }
    let mut templates: Vec<AuthoredTemplate> = Vec::new();
    for recipe in recipes {
        append_templates(&mut templates, recipe);
    }
    (templates, warnings)
}

fn append_templates(templates: &mut Vec<AuthoredTemplate>, recipe: WeaponRecipe) {
    for mut variant in recipe.overrides.socket_plug_variants {
        variant.socket_index = 0;
        variant.choice_index = 0;
        if !templates.iter().any(|template| template.variant == variant) {
            templates.push(AuthoredTemplate {
                weapon: recipe.name.clone(),
                variant,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row> {
        let row = |authored, index, name: &str, type_name: &str, effects| {
            Row::new(
                authored,
                index,
                0x1000 + index as u32,
                name.to_owned(),
                format!("{type_name} · about {name}"),
                type_name.to_owned(),
                effects,
            )
        };
        vec![
            row(false, 0, "Rampage", "Trait", true),
            row(false, 1, "Arrowhead Brake", "Barrel", true),
            row(false, 2, "Bright Ornament", "", false),
            row(false, 3, "Adaptive Frame", "Intrinsic", true),
            row(true, 0, "Zealot's Reward", "Trait", true),
            row(true, 1, "Blank Slate", "Trait", false),
        ]
    }

    fn names(filters: &Filters, rows: &[Row]) -> Vec<String> {
        let mut kept = rows
            .iter()
            .filter(|row| filters.keeps(row))
            .collect::<Vec<_>>();
        kept.sort_by(|a, b| sort_key(filters.order, a).cmp(&sort_key(filters.order, b)));
        kept.iter().map(|row| row.name.clone()).collect()
    }

    #[test]
    fn every_template_order_is_total_and_keeps_the_same_result_set() {
        let rows = rows();
        let all = |order| {
            names(
                &Filters {
                    order,
                    effects: Effects::Any,
                    ..Default::default()
                },
                &rows,
            )
        };
        assert_eq!(
            all(Order::CustomFirst),
            [
                "Blank Slate",
                "Zealot's Reward",
                "Adaptive Frame",
                "Arrowhead Brake",
                "Bright Ornament",
                "Rampage"
            ]
        );
        assert_eq!(
            all(Order::Name),
            [
                "Adaptive Frame",
                "Arrowhead Brake",
                "Blank Slate",
                "Bright Ornament",
                "Rampage",
                "Zealot's Reward"
            ]
        );
        // Type groups, name breaks ties, and a perk with no type label sorts last.
        assert_eq!(
            all(Order::Type),
            [
                "Arrowhead Brake",
                "Adaptive Frame",
                "Blank Slate",
                "Rampage",
                "Zealot's Reward",
                "Bright Ornament"
            ]
        );
        for order in Order::ALL {
            let mut sorted = all(order);
            sorted.sort();
            let mut every = rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();
            every.sort();
            assert_eq!(sorted, every, "{} hid a perk", order.label());
        }
    }

    #[test]
    fn template_filters_read_the_perk_not_where_it_was_read_from() {
        let rows = rows();
        let with = |filters: Filters| names(&filters, &rows);
        assert_eq!(
            with(Filters {
                origin: Origin::Custom,
                effects: Effects::Any,
                ..Default::default()
            }),
            ["Blank Slate", "Zealot's Reward"]
        );
        assert_eq!(
            with(Filters {
                origin: Origin::Stock,
                effects: Effects::Without,
                ..Default::default()
            }),
            ["Bright Ornament"]
        );
        assert_eq!(
            with(Filters {
                type_name: "Trait".into(),
                ..Default::default()
            }),
            ["Zealot's Reward", "Rampage"]
        );
        // The search text carries the name, the detail line and the hash.
        assert!(pickers::matches("about rampage", &rows[0].search));
        assert!(pickers::matches("00001000", &rows[0].search));
        assert!(!pickers::matches("barrel", &rows[0].search));
    }

    fn text_at(output: &egui::FullOutput, name: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.galley.rect.translate(text.pos.to_vec2()).center())
            }
            _ => None,
        })
    }

    fn text_starting(output: &egui::FullOutput, prefix: &str) -> Option<String> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with(prefix) => {
                Some(text.galley.job.text.clone())
            }
            _ => None,
        })
    }

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn the_copy_existing_picker_filters_and_orders_the_stock_perks() {
        let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
        let mut workbench = Workbench {
            initialized: true,
            open: true,
            ..Default::default()
        };
        let weapon = WeaponRecipe::every_end();
        let ctx = egui::Context::default();
        let size = egui::vec2(1000.0, 720.0);
        let run = |workbench: &mut Workbench, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    assert!(
                        workbench
                            .show(
                                ctx,
                                Path::new(""),
                                Some(&catalog),
                                &[],
                                false,
                                (&weapon, None)
                            )
                            .is_none()
                    );
                },
            )
        };
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = run(&mut workbench, vec![]);
        }
        let anchor = text_at(&output, "Copy Existing…").expect("Copy Existing");
        for pressed in [true, false] {
            output = run(
                &mut workbench,
                vec![
                    egui::Event::PointerMoved(anchor),
                    egui::Event::PointerButton {
                        pos: anchor,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        for _ in 0..2 {
            output = run(&mut workbench, vec![]);
        }
        for name in [
            "All Types",
            "Stock or Custom",
            "With Effects",
            "Sort: Custom First",
        ] {
            assert!(text_at(&output, name).is_some(), "missing {name}");
        }
        super::super::tests::capture::write(&ctx, &output, "copy-existing-open");
        let rows = workbench.template_rows.as_deref().unwrap();
        let with_effects = rows.iter().filter(|row| row.effects).count();
        let results = text_starting(&output, &format!("{with_effects} Results"));
        assert!(
            results.is_some() && with_effects < rows.len(),
            "the picker opens on the perks that carry an effect"
        );
        let mut types = BTreeMap::<&str, (usize, usize)>::new();
        for row in rows {
            let entry = types.entry(row.type_name.as_str()).or_default();
            entry.0 += 1;
            if row.effects {
                entry.1 += 1;
            }
        }
        println!("{} stock rows", rows.len());
        for (name, (count, with_effects)) in &types {
            println!("{name:?}: {count} rows, {with_effects} with effects");
        }
        // Narrowing to one type and to perks with effects, through the stored filters,
        // shows fewer rows than the type alone and more than none. The type is the one with
        // the most perks of each kind, so the check does not depend on one install's names.
        let (type_name, (count, _)) = types
            .iter()
            .filter(|(name, _)| !name.is_empty())
            .max_by_key(|(_, (count, with))| (*with).min(count - with))
            .map(|(name, counts)| ((*name).to_owned(), *counts))
            .unwrap();
        let expected = rows
            .iter()
            .filter(|row| row.type_name == type_name && row.effects)
            .count();
        ctx.data_mut(|data| {
            data.insert_temp(
                Filters::id(),
                Filters {
                    type_name: type_name.clone(),
                    ..Default::default()
                },
            );
        });
        let output = run(&mut workbench, vec![]);
        super::super::tests::capture::write(&ctx, &output, "copy-existing-filtered");
        assert!(text_at(&output, &type_name).is_some());
        let shown = text_starting(&output, "").and_then(|_| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.ends_with(" Results") => text
                    .galley
                    .job
                    .text
                    .trim_end_matches(" Results")
                    .parse::<usize>()
                    .ok(),
                _ => None,
            })
        });
        assert_eq!(shown, Some(expected));
        assert!(
            0 < expected && expected < count,
            "{type_name}: {expected} of {count}"
        );
        println!("{type_name}: {expected} of {count} carry effects");
    }

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_LIBRARY_ROOT"]
    fn saved_weapon_perks_migrate_to_files_without_changing_weapons_or_importing_drafts() {
        let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let root = PathBuf::from(std::env::var_os("PARHELION_LIBRARY_ROOT").unwrap());
        let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let weapons = RecipeLibrary::open(temp.path().join("recipes")).unwrap();
        let mut originals = Vec::new();
        for entry in std::fs::read_dir(root.join("recipes")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".parhelion.json") {
                let bytes = std::fs::read(&path).unwrap();
                let copied = weapons.root().join(path.file_name().unwrap());
                std::fs::write(&copied, &bytes).unwrap();
                originals.push((copied, bytes));
            }
        }
        let perks = Library::open(temp.path().join("perks")).unwrap();
        // Seed existing standalone files to exercise duplicate detection across formats.
        for entry in std::fs::read_dir(root.join("perks")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".perk.json") {
                Library::read(&path).unwrap();
                std::fs::copy(&path, perks.root().join(path.file_name().unwrap())).unwrap();
            }
        }
        let initial = perks.scan().unwrap().entries.len();
        let mut workbench = Workbench {
            initialized: true,
            library: Some(perks),
            ..Default::default()
        };
        let mut draft = WeaponRecipe::every_end();
        let mut unsaved = PerkRecipe::new();
        unsaved.name = "Unsaved Draft Must Not Be Imported".into();
        draft.overrides.socket_plug_variants = vec![unsaved.at_socket(0, 0)];
        workbench.load_authored_templates(Some(&weapons), &draft, &catalog);
        assert!(workbench.error.is_none(), "{:?}", workbench.error);
        assert!(
            !workbench
                .entries
                .iter()
                .any(|entry| entry.recipe.name == unsaved.name)
        );
        let count = workbench.entries.len();
        assert!(count >= initial);
        workbench.load_authored_templates(Some(&weapons), &draft, &catalog);
        assert!(workbench.error.is_none(), "{:?}", workbench.error);
        assert_eq!(workbench.entries.len(), count);
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        println!(
            "{initial} existing standalone perks, {} new embedded perks, {count} total. Refresh is idempotent and saved weapons are unchanged.",
            count - initial
        );
    }
}
