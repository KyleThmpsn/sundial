use super::*;
use crate::perk::library::saved::{self, Loaded, SavedPerk, SavedPerks};
use std::collections::BTreeSet;

impl Workbench {
    /// Reads the saved recipes' perks and adds the new ones to Custom Perks before returning. The
    /// window reads them through [`Self::load_templates`] instead.
    pub(super) fn load_authored_templates(
        &mut self,
        library: Option<&RecipeLibrary>,
        draft: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        self.initialize();
        let loaded = saved::load(self.library.as_ref(), library, catalog);
        self.adopt_templates(loaded, draft, catalog);
    }

    /// The ids of the open documents, whose saved files stay while they are open.
    fn open_ids(&self) -> BTreeSet<String> {
        self.documents
            .iter()
            .map(|document| document.recipe.id.clone())
            .collect()
    }

    /// Keeps the saved recipes' perks loaded while the workbench or Select Custom Perk is open. A
    /// large library takes seconds to read and import, so a worker does it while the window draws,
    /// and a picker opened meanwhile fills in when it lands. Without saved recipes there is nothing
    /// slow to read, so they load at once.
    pub(super) fn load_templates(
        &mut self,
        ctx: &egui::Context,
        library: Option<&RecipeLibrary>,
        draft: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        if let Some((generation, receiver)) = self.template_load.take() {
            match receiver.try_recv() {
                Ok(loaded) if generation == self.template_generation => {
                    self.adopt_templates(loaded, draft, catalog);
                }
                // Perks read before the saved recipes or the catalog changed are read again.
                Ok(_) => {}
                Err(TryRecvError::Empty) => {
                    self.template_load = Some((generation, receiver));
                    return;
                }
                // A worker that stopped is not started again until the recipes change.
                Err(TryRecvError::Disconnected) => {
                    self.authored_templates = Some(Vec::new());
                    self.template_browse = Browse::default();
                    self.error = Some("Could not read the perks of saved recipes.".into());
                }
            }
            self.fill_picker(draft, catalog);
        }
        if self.authored_templates.is_some() {
            self.follow_draft(draft, catalog);
            return;
        }
        let Some(library) = library else {
            self.load_authored_templates(None, draft, catalog);
            self.fill_picker(draft, catalog);
            return;
        };
        self.initialize();
        let perks = self.library.clone();
        let recipes = library.clone();
        let catalog = catalog.clone();
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            if sender
                .send(saved::load(perks.as_ref(), Some(&recipes), &catalog))
                .is_ok()
            {
                repaint.request_repaint();
            }
        });
        self.template_load = Some((self.template_generation, receiver));
    }

    /// Whether the saved recipes' perks are still being read.
    pub(super) fn loading_templates(&self) -> bool {
        self.template_load.is_some()
    }

    /// Forgets the saved recipes' perks so they are read again. Perks still being read are
    /// dropped when they land, and the worker keeps the library to itself until then.
    pub(super) fn forget_authored_templates(&mut self) {
        self.authored_templates = None;
        self.template_draft = None;
        self.template_browse = Browse::default();
        self.template_generation = self.template_generation.wrapping_add(1);
    }

    /// Keeps the open recipe's perks among the authored templates as `draft` holds them now. A
    /// saved recipe's perk that equals one stays the saved recipe's.
    fn follow_draft(&mut self, draft: &WeaponRecipe, catalog: &InvestmentCatalog) {
        let current = |(namespace, variants): &(String, Vec<WeaponSocketPlugVariantRecipe>)| {
            *namespace == draft.namespace && *variants == draft.overrides.socket_plug_variants
        };
        if self.template_draft.as_ref().is_some_and(current) {
            return;
        }
        let Some(list) = &mut self.authored_templates else {
            return;
        };
        list.retain(|template| !template.draft);
        let fresh = SavedPerks::of_draft(draft, catalog)
            .into_iter()
            .filter(|template| !list.iter().any(|saved| saved.variant == template.variant))
            .collect::<Vec<_>>();
        list.extend(fresh);
        self.template_browse = Browse::default();
        self.template_draft = Some((
            draft.namespace.clone(),
            draft.overrides.socket_plug_variants.clone(),
        ));
    }

    /// Takes the perks read from saved recipes. The imports they replaced go, except the perks
    /// open now, Custom Perks is read again, and the open recipe's own perks join them.
    fn adopt_templates(
        &mut self,
        loaded: Loaded,
        draft: &WeaponRecipe,
        catalog: &InvestmentCatalog,
    ) {
        let Loaded {
            mut perks,
            added,
            redundant,
            library: read,
            mut warnings,
        } = loaded;
        // The worker read the library after importing. Unless a perk file changed since, that
        // read stands, and the window does not read every file again.
        let read = read.filter(|(scan, _)| {
            self.library
                .as_ref()
                .is_some_and(|library| library.unchanged_since(scan))
        });
        let retired = match &self.library {
            Some(library) if !redundant.is_empty() => {
                match library.remove_unchanged(&redundant, &self.open_ids()) {
                    Ok(removed) => removed,
                    Err(error) => {
                        warnings.push(error);
                        Vec::new()
                    }
                }
            }
            _ => Vec::new(),
        };
        let plural = |count: usize| if count == 1 { "" } else { "s" };
        let mut notes = Vec::new();
        if added > 0 {
            notes.push(format!(
                "Added {added} custom perk{} from saved recipes to the library.",
                plural(added)
            ));
        }
        if !retired.is_empty() {
            notes.push(format!(
                "Removed {} imported perk{} that saved recipes replaced.",
                retired.len(),
                plural(retired.len())
            ));
        }
        if !notes.is_empty() {
            self.message = Some(notes.join(" "));
        }
        match read {
            Some((scan, history)) => {
                self.entries = scan
                    .entries
                    .into_iter()
                    .filter(|entry| !retired.contains(&entry.path))
                    .collect();
                match history {
                    Ok(ids) => self.imported_perks = ids,
                    Err(error) => warnings.push(error),
                }
            }
            None if self.library.is_some() => warnings.extend(self.scan_library()),
            None => {}
        }
        perks.append_draft(draft, catalog);
        self.authored_templates = Some(perks.list);
        self.template_browse = Browse::default();
        self.template_draft = Some((
            draft.namespace.clone(),
            draft.overrides.socket_plug_variants.clone(),
        ));
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
    }

    pub(super) fn draw_templates(&mut self, ui: &mut egui::Ui, catalog: &InvestmentCatalog) {
        let stock = self.templates.get_or_insert_with(|| {
            catalog.perk_template_choices_from(crate::package_profile::is_stock_item_definition)
        });
        // The stock rows read a description and an effect list per perk, so they are built
        // once per catalog, and the authored rows once per change to the saved perks. A library
        // of imported weapons holds thousands, so the listing is filtered and sorted again only
        // when the query or a filter changes.
        let stock_rows = self.template_rows.get_or_insert_with(|| {
            stock
                .iter()
                .enumerate()
                .map(|(index, choice)| Row::stock(index, choice, catalog))
                .collect()
        });
        let authored = self.authored_templates.as_deref().unwrap_or_default();
        let browse = &mut self.template_browse;
        let authored_rows = browse.authored.get_or_insert_with(|| {
            authored
                .iter()
                .enumerate()
                .map(|(index, entry)| Row::authored(index, entry, catalog))
                .collect()
        });
        // The type list is read off the rows, so it offers only types some perk has.
        let types = browse.types.get_or_insert_with(|| {
            authored_rows
                .iter()
                .chain(stock_rows.iter())
                .map(|row| row.type_name.as_str())
                .filter(|name| !name.is_empty())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(str::to_owned)
                .collect()
        });
        let (authored_rows, stock_rows): (&[Row], &[Row]) = (authored_rows, stock_rows);
        let listing = &mut browse.listing;
        let picked = pickers::browser_with_toolbar(
            ui,
            "perk-template",
            "New from Perk…",
            "New from Perk",
            &mut self.template_query,
            |ui, query, opened, _| {
                let types = types.iter().map(String::as_str).collect::<Vec<_>>();
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
                if listing
                    .as_ref()
                    .is_some_and(|listing| listing.words != words || listing.filters != filters)
                {
                    *listing = None;
                }
                let listing = &*listing
                    .get_or_insert_with(|| Listing::new(authored_rows, stock_rows, words, filters));
                let row = |index: usize| {
                    let (authored, at) = listing.rows[index];
                    if authored {
                        &authored_rows[at]
                    } else {
                        &stock_rows[at]
                    }
                };
                pickers::BrowserList {
                    keys: &listing.keys,
                    // The result count takes a line above the list.
                    height: (ui.available_height() - 24.0).max(160.0),
                    reset: opened || searched || filtered,
                    row_height: sundial::investment::authoring_choice_row_height(ui),
                    select: None,
                }
                .draw_activating(
                    ui,
                    |ui, index, selected| {
                        let row = row(index);
                        catalog.draw_authoring_choice_row(
                            ui,
                            Some(row.hash),
                            &row.name,
                            Some(&row.detail),
                            selected,
                        )
                    },
                    |ui, index, activated| {
                        let row = row(index);
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
            // Each saved perk is resolved once, so every copy takes a document identity of its own.
            let mut recipe = authored[index].perk.clone();
            recipe.id = PerkRecipe::new().id;
            recipe
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
    authored: &[SavedPerk],
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

    fn authored(index: usize, entry: &SavedPerk, catalog: &InvestmentCatalog) -> Self {
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

/// What the New from Perk picker keeps between frames: the authored rows and the type list,
/// built once per change to the saved perks, and the listing for the last query and filters.
#[derive(Default)]
pub(super) struct Browse {
    authored: Option<Vec<Row>>,
    types: Option<Vec<String>>,
    listing: Option<Listing>,
}

/// The rows a query and filters keep, in order, as each row's origin and index.
struct Listing {
    words: String,
    filters: Filters,
    rows: Vec<(bool, usize)>,
    /// Saved and stock rows count their indices separately, so the origin keeps each key apart.
    keys: Vec<u64>,
}

impl Listing {
    fn new(authored: &[Row], stock: &[Row], words: String, filters: Filters) -> Self {
        let mut rows = authored
            .iter()
            .chain(stock)
            .filter(|row| {
                filters.keeps(row)
                    && words
                        .split_whitespace()
                        .all(|word| row.search.contains(word))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| sort_key(filters.order, a).cmp(&sort_key(filters.order, b)));
        let keys = rows
            .iter()
            .map(|row| (u64::from(row.authored) << 32) | row.index as u64)
            .collect();
        let rows = rows.iter().map(|row| (row.authored, row.index)).collect();
        Self {
            words,
            filters,
            rows,
            keys,
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
        let (embedded, warnings) = SavedPerks::read(Some(&weapons), &catalog);
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
