use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct AuthoredTemplate {
    pub(super) weapon: String,
    pub(super) kind: crate::ItemKind,
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
            let recipes = templates.list.iter().filter_map(|entry| {
                let hash = entry.variant.source_plug_hash.parse_u32().unwrap_or_default();
                if !entry.variant.replace_effects && catalog.item_definition_tag(hash).is_none() {
                    warnings.push(format!("Could not import a custom perk from {} because its stock template {hash:08X} is unavailable. The recipe is unchanged.", entry.weapon));
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
                            "Added {} custom perk{} from saved recipes to the library.",
                            report.added,
                            if report.added == 1 { "" } else { "s" }
                        ));
                    }
                }
                Err(error) => warnings.push(error),
            }
            warnings.extend(self.scan_library());
        }
        templates.append_draft(draft);
        self.authored_templates = Some(templates.list);
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
        // read each frame the picker is open.
        let stock_rows = self.template_rows.get_or_insert_with(|| {
            stock
                .iter()
                .enumerate()
                .map(|(index, choice)| Row::stock(index, choice, catalog))
                .collect()
        });
        let authored = self.authored_templates.as_deref().unwrap_or_default();
        let picked = pickers::browser_with_toolbar(
            ui,
            "perk-template",
            "New from Perk…",
            "New from Perk",
            &mut self.template_query,
            |ui, query, opened, _| {
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
                let searched = ui
                    .horizontal(|ui| {
                        let width = (ui.available_width() - pickers::CLEAR_WIDTH).max(160.0);
                        sundial::ui::catalog::search(ui, query, opened, width, "Search Perks")
                    })
                    .inner;
                let mut filters = Filters::load(ui);
                let filtered = filters.toolbar(ui, &types);
                filters.store(ui);
                // Each row's search text is stored lowercase.
                let words = query.to_lowercase();
                let mut rows = authored_rows
                    .iter()
                    .chain(stock_rows.iter())
                    .filter(|row| {
                        filters.keeps(row)
                            && words
                                .split_whitespace()
                                .all(|word| row.search.contains(word))
                    })
                    .collect::<Vec<_>>();
                rows.sort_by(|a, b| sort_key(filters.order, a).cmp(&sort_key(filters.order, b)));
                // Saved and stock rows count their indices separately, so the origin keeps
                // each key apart.
                let keys = rows
                    .iter()
                    .map(|row| (u64::from(row.authored) << 32) | row.index as u64)
                    .collect::<Vec<_>>();
                pickers::BrowserList {
                    keys: &keys,
                    // The result count takes a line above the list.
                    height: (ui.available_height() - 24.0).max(160.0),
                    reset: opened || searched || filtered,
                    row_height: sundial::investment::authoring_choice_row_height(ui),
                    select: None,
                }
                .draw_activating(
                    ui,
                    |ui, index, selected| {
                        let row = rows[index];
                        catalog.draw_authoring_choice_row(
                            ui,
                            Some(row.hash),
                            &row.name,
                            Some(&row.detail),
                            selected,
                        )
                    },
                    |ui, index, activated| {
                        let row = rows[index];
                        ui.heading(&row.name);
                        // A double-click copies the perk, as Copy Perk does.
                        let copy = ui
                            .add(crate::app::style::primary(ui, "Copy Perk"))
                            .clicked()
                            || activated;
                        draw_template_facts(ui, row, authored, catalog);
                        copy.then_some((row.authored, row.index))
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

/// What a New from Perk detail shows under Copy Perk: the type and hash, where the perk comes
/// from and whether it carries effects, then its description.
fn draw_template_facts(
    ui: &mut egui::Ui,
    row: &Row,
    authored: &[AuthoredTemplate],
    catalog: &InvestmentCatalog,
) {
    let secondary = crate::app::style::secondary(ui.visuals());
    let kind = if row.type_name.is_empty() {
        format!("0x{:08X}", row.hash)
    } else {
        format!("{} · 0x{:08X}", row.type_name, row.hash)
    };
    ui.label(egui::RichText::new(kind).color(secondary));
    let stock_description = || {
        catalog
            .perk_description(row.hash)
            .unwrap_or_default()
            .to_owned()
    };
    let (origin, description) = if row.authored {
        let entry = &authored[row.index];
        (
            format!("Custom · {}", entry.weapon),
            entry
                .variant
                .description
                .clone()
                .unwrap_or_else(stock_description),
        )
    } else {
        ("Stock".to_owned(), stock_description())
    };
    let effects = if row.effects {
        Effects::With
    } else {
        Effects::Without
    };
    ui.label(egui::RichText::new(format!("{origin} · {}", effects.label())).color(secondary));
    if !description.is_empty() {
        ui.add_space(6.0);
        ui.label(crate::app::style::destiny_text(ui, description));
    }
}

/// One row of the New from Perk picker, with the facts its filters and orders read: the
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
    /// What the search box matches against, lowercase: the name, the detail line and the hash.
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
            search: format!("{name} {detail} {hash:08X}").to_lowercase(),
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
            Self::Any => "Every perk, whether the game shipped it or a saved recipe carries it.",
            Self::Stock => "Perks the game ships.",
            Self::Custom => "Perks saved with your recipes, and the ones on the item being edited.",
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

/// The picker's filters and order, kept in egui memory so they outlive the window.
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

    /// Draws the filter and sort controls. Returns whether a choice changed.
    fn toolbar(&mut self, ui: &mut egui::Ui, types: &[&str]) -> bool {
        let before = self.clone();
        // A type no perk has any more reads as every type rather than as an empty list.
        if !self.type_name.is_empty() && !types.contains(&self.type_name.as_str()) {
            self.type_name.clear();
        }
        ui.horizontal_wrapped(|ui| {
            // Four combos do not fit one line in a narrow window. Each is held to one width
            // and truncates to it, with its full reading on hover, and one that would run
            // past the edge starts the next line instead.
            const FILTER_WIDTH: f32 = 150.0;
            let fit = |ui: &mut egui::Ui| {
                if ui.available_rect_before_wrap().width()
                    < FILTER_WIDTH + ui.spacing().item_spacing.x
                {
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
                    .on_hover_text(format!("Perk Type: {type_text}"));
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
                    .on_hover_text(format!("Origin: {origin_text}"));
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
                    .on_hover_text(format!("Effects: {effects_text}"));
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
                    .on_hover_text(order_text.clone());
                pickers::name_combo(ui, "perk-template-order", "Sort Order");
            });
        });
        *self != before
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
    recipe.offer_everywhere = variant.offer_everywhere;
    recipe.projectile = variant.projectile;
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
    templates.append_draft(draft);
    (templates.list, warnings)
}

/// How many saved recipes are held at once while their templates are collected.
const RECIPES_AT_ONCE: usize = 64;

fn load_saved(library: Option<&RecipeLibrary>) -> (Templates, Vec<String>) {
    let mut warnings = Vec::new();
    let mut templates = Templates::default();
    if let Some(library) = library {
        match library.scan() {
            Ok(scan) => {
                warnings.extend(scan.errors);
                let paths: Vec<_> = scan.entries.into_iter().map(|entry| entry.path).collect();
                for chunk in paths.chunks(RECIPES_AT_ONCE) {
                    let recipes = crate::recipe_library::load_listings(chunk)
                        .into_iter()
                        .filter_map(|recipe| {
                            recipe
                                .map_err(|error| warnings.push(error.to_string()))
                                .ok()
                        })
                        .collect::<Vec<_>>();
                    let linked = LinkedIcons::read(&recipes);
                    for recipe in recipes {
                        templates.append(recipe, &linked);
                    }
                }
            }
            Err(error) => warnings.push(error),
        }
    }
    (templates, warnings)
}

/// The authored templates, each variant once, in the order they were first found.
#[derive(Default)]
struct Templates {
    list: Vec<AuthoredTemplate>,
    /// Each template's index by its stock plug and name. A library of imported weapons holds
    /// tens of thousands of variants, too many to compare each with every one before it.
    by_plug: BTreeMap<(crate::recipe::HexHash, Option<String>), Vec<usize>>,
}

/// The icons of some recipes' linked ornaments, which the importer keeps in each ornament's
/// conversion rather than in the weapon's variant, by the item hash of each private plug.
#[derive(Default)]
struct LinkedIcons(
    #[cfg_attr(not(feature = "d2-model-importer"), allow(dead_code))]
    BTreeMap<u32, crate::icon_edit::ImportedIcon>,
);

impl LinkedIcons {
    #[cfg(feature = "d2-model-importer")]
    fn read(recipes: &[WeaponRecipe]) -> Self {
        Self(crate::imported::ornament_icons(
            recipes
                .iter()
                .filter_map(|recipe| recipe.overrides.imported_graph.as_ref())
                .flat_map(|graph| &graph.attachments),
        ))
    }

    #[cfg(not(feature = "d2-model-importer"))]
    fn read(_: &[WeaponRecipe]) -> Self {
        Self::default()
    }

    /// The icon of the linked ornament whose private plug is a recipe's choice at `socket`.
    #[cfg(feature = "d2-model-importer")]
    fn get(
        &self,
        namespace: &str,
        socket: u16,
        choice: u16,
    ) -> Option<crate::icon_edit::ImportedIcon> {
        use parhelion_import::tiger::ornament::{fnv, roles};
        let roles = roles(namespace, usize::from(socket), usize::from(choice));
        self.0.get(&fnv(&format!("{roles}/private-plug"))).cloned()
    }

    #[cfg(not(feature = "d2-model-importer"))]
    fn get(&self, _: &str, _: u16, _: u16) -> Option<crate::icon_edit::ImportedIcon> {
        None
    }
}

impl Templates {
    fn append_draft(&mut self, draft: &WeaponRecipe) {
        let linked = LinkedIcons::read(std::slice::from_ref(draft));
        self.append(draft.clone(), &linked);
    }

    fn append(&mut self, recipe: WeaponRecipe, linked: &LinkedIcons) {
        // A mod's plug is the item, so it shows the item's own image.
        let item_image = (recipe.kind == crate::ItemKind::Mod)
            .then(|| recipe.overrides.icon_edit.imported_image.clone())
            .flatten();
        for mut variant in recipe.overrides.socket_plug_variants {
            // An ornament's plug shows an image its variant does not hold: a mod's item image, or
            // the icon of a weapon's linked ornament conversion.
            if variant.icon.is_none() {
                variant.icon = item_image
                    .clone()
                    .or_else(|| {
                        linked.get(
                            &recipe.namespace,
                            variant.socket_index,
                            variant.choice_index,
                        )
                    })
                    .map(|image| crate::perk::Icon::Image {
                        name: variant.name.clone().unwrap_or_else(|| recipe.name.clone()),
                        image,
                    });
            }
            variant.socket_index = 0;
            variant.choice_index = 0;
            let same = self
                .by_plug
                .entry((variant.source_plug_hash.clone(), variant.name.clone()))
                .or_default();
            if same
                .iter()
                .any(|&index| self.list[index].variant == variant)
            {
                continue;
            }
            same.push(self.list.len());
            self.list.push(AuthoredTemplate {
                weapon: recipe.name.clone(),
                kind: recipe.kind,
                variant,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires SUNDIAL_STOCK_PACKAGES and PARHELION_LIBRARY_ROOT"]
    fn saved_weapon_perks_migrate_to_files_without_changing_weapons_or_importing_drafts() {
        let packages = crate::test_support::stock_packages();
        let root = PathBuf::from(std::env::var_os("PARHELION_LIBRARY_ROOT").unwrap());
        let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
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
        assert!(
            !originals.is_empty(),
            "no saved weapon recipes were configured"
        );
        let (embedded, warnings) = load_saved(Some(&weapons));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            !embedded.list.is_empty(),
            "no embedded saved perks were configured"
        );
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
