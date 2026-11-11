//! Add from Perk: the stock effects the filters leave, listed as one row per perk that opens to
//! its effects, and the detail pane that adds them.
use super::super::assets::variants;
use super::*;

/// What the picker reads its rows from.
pub(super) struct Perks<'a> {
    pub(super) choices: &'a [WeaponSandboxPerkChoice],
    pub(super) names: &'a BTreeMap<u16, String>,
    pub(super) discovery: &'a discovery::Discovery,
    pub(super) ingredients: Option<&'a Arc<sundial::investment::IngredientCatalog>>,
    pub(super) catalog: Option<&'a InvestmentCatalog>,
    pub(super) asset_labels: &'a BTreeMap<u32, String>,
}

/// The toolbar's choices, which the workbench keeps between frames.
pub(super) struct Toolbar<'a> {
    pub(super) purpose: &'a mut guidance::Purpose,
    pub(super) editing: &'a mut guidance::EditingFilter,
    pub(super) source: &'a mut Option<sundial::investment::IngredientSource>,
    pub(super) order: &'a mut guidance::EffectOrder,
}

/// The toolbar's choices, as the effects are filtered by them.
pub(super) struct Filter<'a> {
    /// The search, trimmed and lowercased.
    query: &'a str,
    show_all: bool,
    purpose: guidance::Purpose,
    editing: guidance::EditingFilter,
    source: Option<sundial::investment::IngredientSource>,
}

/// The listed perks: their effects in list order, and each perk as the positions of its
/// effects there.
pub(super) struct Listing<'a> {
    effects: Vec<&'a WeaponSandboxPerkChoice>,
    families: Vec<variants::Group>,
}

/// Whether the perk already holds the stock effect at `index`. An authored program on the same
/// index does not count.
pub(super) fn included(recipe: &PerkRecipe, index: u16) -> bool {
    recipe
        .effects
        .iter()
        .any(|effect| effect.program.is_none() && effect.source_perk_index == index)
}

impl<'a> Perks<'a> {
    fn sources(&self, index: u16) -> Option<&'a BTreeSet<sundial::investment::IngredientSource>> {
        self.ingredients.and_then(|data| data.sources.get(&index))
    }

    /// The effects `filter` leaves, in the order the catalog lists them.
    pub(super) fn available(&self, filter: &Filter) -> Vec<&'a WeaponSandboxPerkChoice> {
        self.choices
            .iter()
            .filter(|choice| self.admits(choice, filter))
            .collect()
    }

    fn admits(&self, choice: &WeaponSandboxPerkChoice, filter: &Filter) -> bool {
        let index = choice.perk_index;
        let behavior = self.discovery.behavior(index);
        let sources = self.sources(index);
        let identified = behavior.is_some_and(|behavior| !behavior.effect_kinds.is_empty())
            && !choice.representative_name.starts_with("Ability ")
            && !choice.representative_name.starts_with("Effect ");
        if !(filter.show_all || identified)
            || !filter
                .source
                .is_none_or(|source| sources.is_some_and(|sources| sources.contains(&source)))
            || !filter.editing.allows(behavior)
        {
            return false;
        }
        // The behavior text is built once, and only when something reads it.
        let summary = if filter.purpose == guidance::Purpose::All && filter.query.is_empty() {
            None
        } else {
            behavior.map(guidance::behavior_search)
        };
        if !filter.purpose.allows_search(summary.as_deref()) {
            return false;
        }
        filter.query.is_empty()
            || pickers::matches(
                filter.query,
                &self.search_text(choice, &summary.unwrap_or_default()),
            )
    }

    /// Everything a search reads of one effect: its name and number, what references it, its
    /// description and behavior, what carries it and what it may need.
    fn search_text(&self, choice: &WeaponSandboxPerkChoice, summary: &str) -> String {
        let index = choice.perk_index;
        let description = self
            .catalog
            .and_then(|catalog| catalog.perk_component_description(index))
            .unwrap_or_default();
        let source_text = self
            .sources(index)
            .map(|sources| {
                sources
                    .iter()
                    .map(|source| source.label())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let context = self
            .ingredients
            .and_then(|data| data.context.get(&index))
            .map(|context| context.iter().cloned().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let details = self
            .ingredients
            .map(|data| data.references.details(usize::from(index)))
            .unwrap_or_default();
        format!(
            "{} {index} {details} {description} {summary} {source_text} {context}",
            self.names[&index]
        )
    }

    /// The perks of the `available` effects in `order`, each where its first effect falls.
    pub(super) fn listing(
        &self,
        mut available: Vec<&'a WeaponSandboxPerkChoice>,
        query: &str,
        order: guidance::EffectOrder,
    ) -> Listing<'a> {
        available.sort_by_cached_key(|choice| {
            let name = self.names[&choice.perk_index].to_lowercase();
            // A perk-name match comes before incidental native field/context matches, such as
            // Fourth Float plus Extend Timers.
            let direct = pickers::matches(query, &name);
            let rank =
                guidance::effect_rank(&choice.representative_name, self.sources(choice.perk_index));
            guidance::effect_sort_key(order, &choice.representative_type_name, &name, direct, rank)
        });
        self.families(&available)
    }

    /// A perk's effects are one row that adds them all, as Demolitionist's two are. The row
    /// gathers every effect its plug carries, so a search or filter that matches one still adds
    /// the whole perk, and opening it lists each effect to add on its own.
    fn families(&self, available: &[&'a WeaponSandboxPerkChoice]) -> Listing<'a> {
        let mut plugs = BTreeMap::<u32, Vec<&'a WeaponSandboxPerkChoice>>::new();
        for choice in self.choices {
            plugs
                .entry(choice.representative_hash)
                .or_default()
                .push(choice);
        }
        let mut listing = Listing {
            effects: Vec::new(),
            families: Vec::new(),
        };
        let mut listed = BTreeSet::new();
        for choice in available {
            if !listed.insert(choice.representative_hash) {
                continue;
            }
            let start = listing.effects.len();
            listing
                .effects
                .extend(plugs[&choice.representative_hash].iter().copied());
            listing.families.push(variants::Group {
                name: choice.representative_name.clone(),
                key: u64::from(choice.representative_hash),
                members: (start..listing.effects.len()).collect(),
            });
        }
        listing
    }

    /// A row's title and the line under it.
    fn row_text(&self, row: variants::Shown, choice: &WeaponSandboxPerkChoice) -> (String, String) {
        let name = self.names[&choice.perk_index].clone();
        let technical = self.technical(choice, &name);
        match row {
            variants::Shown::Family(family, _) => (
                family.name.clone(),
                format!(
                    "{} Effects · {}",
                    family.members.len(),
                    choice.representative_type_name
                ),
            ),
            variants::Shown::Variant(_) => self.variant_text(choice, technical),
            variants::Shown::Asset(_) => (name, technical),
        }
    }

    /// The effect's identity, the line under its name.
    fn technical(&self, choice: &WeaponSandboxPerkChoice, name: &str) -> String {
        let technical = self
            .discovery
            .data
            .as_ref()
            .and_then(|data| data.perks.perks.get(usize::from(choice.perk_index)))
            .map(reading::identity)
            .unwrap_or_else(|| format!("Effect {}", choice.perk_index));
        // A shared name already carries the effect number, so the line under it does not
        // repeat it.
        let number = format!(" · Effect {}", choice.perk_index);
        if name.ends_with(&number) {
            technical
                .strip_suffix(&number)
                .unwrap_or(&technical)
                .to_owned()
        } else {
            technical
        }
    }

    /// One of a perk's effects reads by what it does, with its number under it, since the
    /// number alone tells a reader nothing.
    fn variant_text(
        &self,
        choice: &WeaponSandboxPerkChoice,
        technical: String,
    ) -> (String, String) {
        let number = format!("Effect {}", choice.perk_index);
        let kinds = technical
            .strip_suffix(&format!(" · {number}"))
            .filter(|kinds| !kinds.is_empty())
            .map(str::to_owned);
        match kinds {
            Some(kinds) => (kinds, number),
            None => (
                number,
                self.discovery
                    .behavior(choice.perk_index)
                    .map_or(technical, |behavior| behavior.headline.clone()),
            ),
        }
    }
}

/// The search box on a line of its own at the picker's width, then the filters, Show All and
/// the result count on a line that wraps in a narrow window. `collect` lists the effects the
/// filters leave, which the count reads. Returns whether the picker opened or a filter or the
/// query changed, and those effects.
pub(super) fn toolbar<'c>(
    ui: &mut egui::Ui,
    query: &mut String,
    opened: bool,
    mut toolbar: Toolbar,
    busy: bool,
    collect: impl FnOnce(&Filter) -> Vec<&'c WeaponSandboxPerkChoice>,
) -> (bool, Vec<&'c WeaponSandboxPerkChoice>) {
    let mut changed = opened;
    changed |= controls::search_line(ui, |ui, width| pickers::search(ui, query, opened, width));
    let (filtered, available) = ui
        .horizontal_wrapped(|ui| {
            let mut changed = filters(ui, &mut toolbar);
            let (show_all, visibility_changed) = pickers::show_all(ui, "existing-behavior");
            changed |= visibility_changed;
            let query = query.trim().to_lowercase();
            let available = collect(&Filter {
                query: &query,
                show_all,
                purpose: *toolbar.purpose,
                editing: *toolbar.editing,
                source: *toolbar.source,
            });
            ui.label(sundial::ui::catalog::result_count(available.len()));
            if busy {
                ui.spinner();
            }
            (changed, available)
        })
        .inner;
    (changed | filtered, available)
}

/// Every filter on this line is held to one width, the same one the behavior picker uses, so
/// the two toolbars read alike. Returns whether a choice changed.
fn filters(ui: &mut egui::Ui, toolbar: &mut Toolbar) -> bool {
    let width = guidance::FILTER_WIDTH;
    let mut changed = guidance::filters(ui, toolbar.purpose, toolbar.editing, width);
    let source_label = toolbar
        .source
        .map_or("All Sources", |source| source.label());
    changed |= controls::filter(
        ui,
        ("ingredient-source", "Source"),
        width,
        (source_label, &format!("Source: {source_label}")),
        toolbar.source,
        std::iter::once((None, "All Sources", "")).chain(
            sundial::investment::IngredientSource::ALL
                .into_iter()
                .map(|source| (Some(source), source.label(), "")),
        ),
    );
    let order_text = format!("Sort: {}", toolbar.order.label());
    changed |= controls::filter(
        ui,
        ("effect-order", "Sort Order"),
        width,
        (&order_text, &order_text),
        toolbar.order,
        guidance::EffectOrder::ALL
            .into_iter()
            .map(|choice| (choice, choice.label(), choice.hint())),
    );
    changed
}

/// Until the perks are read there is nothing to list, which is not the same as nothing
/// matching. Shows why the read failed or that it is running, and returns whether it did.
pub(super) fn waiting(ui: &mut egui::Ui, discovery: &discovery::Discovery) -> bool {
    if discovery.data.is_some() {
        return false;
    }
    match &discovery.error {
        Some(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading…");
            });
        }
    }
    true
}

/// The perks and the detail pane of the selected row. Returns the effects to add.
pub(super) fn list(
    ui: &mut egui::Ui,
    perks: &Perks,
    listing: &Listing,
    recipe: &PerkRecipe,
    reset: bool,
) -> Option<Vec<u16>> {
    let open_id = ui.make_persistent_id("perk-families-open");
    let open = variants::opened(ui.ctx(), open_id);
    let shown = variants::shown(&listing.families, &open);
    let keys = shown
        .iter()
        .map(|row| row.key(|position| u32::from(listing.effects[position].perk_index)))
        .collect::<Vec<_>>();
    let mut toggled = None;
    let picked = controls::browser_list(ui, &keys, reset, None).draw_body_activating(
        ui,
        |ui, index, selected| draw_row(ui, perks, listing, shown[index], selected, &mut toggled),
        |ui, index, activated| details(ui, perks, listing, shown[index], recipe, activated),
        0,
    );
    if let Some(key) = toggled {
        variants::toggle(ui.ctx(), open_id, key);
    }
    picked
}

/// One row: a perk that opens to its effects, one of an open perk's effects, or an effect
/// alone. A perk's row sets `toggled` to its key when a click or the keyboard opens or closes
/// it.
fn draw_row(
    ui: &mut egui::Ui,
    perks: &Perks,
    listing: &Listing,
    row: variants::Shown,
    selected: bool,
    toggled: &mut Option<u64>,
) -> egui::Response {
    let choice = listing.effects[row.position()];
    let (title, detail) = perks.row_text(row, choice);
    let draw = |ui: &mut egui::Ui| {
        if let Some(catalog) = perks.catalog {
            catalog.draw_authoring_choice_row(
                ui,
                Some(choice.representative_hash),
                &title,
                Some(&detail),
                selected,
            )
        } else {
            sundial::investment::draw_asset_choice_row(ui, &title, &detail, selected)
        }
    };
    let response = match row {
        variants::Shown::Variant(_) => variants::indented(ui, draw),
        variants::Shown::Family(..) => variants::with_caret_room(ui, draw),
        variants::Shown::Asset(_) => draw(ui),
    };
    if let variants::Shown::Family(family, opened) = row {
        variants::paint_caret(ui, &response, opened, selected);
        // A double-click adds the perk, so its second click leaves the row as the first set it.
        if (response.clicked() && !(response.double_clicked() && selected))
            || variants::keyboard_toggle(ui, selected, opened)
        {
            *toggled = Some(family.key);
        }
    }
    response
}

/// The effects a row stands for: a perk's, or the one effect.
fn members<'a>(listing: &Listing<'a>, row: variants::Shown) -> Vec<&'a WeaponSandboxPerkChoice> {
    match row {
        variants::Shown::Family(family, _) => family
            .members
            .iter()
            .map(|&position| listing.effects[position])
            .collect(),
        _ => vec![listing.effects[row.position()]],
    }
}

/// The add command for a row of `all` effects, `missing` of them not yet in the perk.
fn add_label(all: usize, missing: usize) -> String {
    match (all, missing) {
        (_, 0) => "Already Added".to_owned(),
        (1, _) => "Add to Perk".to_owned(),
        (all, count) if count == all => format!("Add {all} Effects"),
        (_, 1) => "Add 1 More Effect".to_owned(),
        (_, count) => format!("Add {count} More Effects"),
    }
}

/// The selected row's command, then what its effects do and what carries them. A perk adds
/// each of its effects not already in this one, and a double-click adds them as the button
/// does. Returns the effects to add.
fn details(
    ui: &mut egui::Ui,
    perks: &Perks,
    listing: &Listing,
    row: variants::Shown,
    recipe: &PerkRecipe,
    activated: bool,
) -> Option<Vec<u16>> {
    let members = members(listing, row);
    let first = members[0];
    ui.heading(match row {
        variants::Shown::Family(family, _) => family.name.clone(),
        _ => perks.names[&first.perk_index].clone(),
    });
    let missing = members
        .iter()
        .map(|choice| choice.perk_index)
        .filter(|index| !included(recipe, *index))
        .collect::<Vec<_>>();
    let issue = members
        .iter()
        .find_map(|choice| perks.discovery.perk_issue(choice.perk_index));
    let command = add_label(members.len(), missing.len());
    let enabled = issue.is_none() && !missing.is_empty();
    if controls::command(ui, enabled, &command, None, activated) {
        return Some(missing);
    }
    if let Some(issue) = issue {
        ui.colored_label(ui.visuals().error_fg_color, issue);
    }
    description(ui, perks.catalog, first);
    readings(ui, perks, &members);
    draw_sources(ui, perks, &members, first);
    None
}

/// The first effect's description, when it has one.
fn description(
    ui: &mut egui::Ui,
    catalog: Option<&InvestmentCatalog>,
    first: &WeaponSandboxPerkChoice,
) {
    if let Some(description) = catalog
        .and_then(|catalog| catalog.perk_component_description(first.perk_index))
        .filter(|description| !description.is_empty())
    {
        ui.separator();
        ui.label(description);
    }
}

/// Each effect's overview, with its full reading under Advanced. A row of several effects
/// names each by its number.
fn readings(ui: &mut egui::Ui, perks: &Perks, members: &[&WeaponSandboxPerkChoice]) {
    for choice in members {
        let Some(behavior) = perks.discovery.behavior(choice.perk_index) else {
            continue;
        };
        ui.separator();
        if members.len() > 1 {
            ui.strong(format!("Effect {}", choice.perk_index));
        }
        reading::overview(ui, behavior, perks.asset_labels);
        egui::CollapsingHeader::new("Advanced")
            .id_salt(("effect-advanced", choice.perk_index))
            .show(ui, |ui| {
                reading::draw(
                    ui,
                    behavior,
                    perks.discovery.data.as_ref(),
                    choice.perk_index,
                    perks.asset_labels,
                );
            });
    }
}

/// What carries the first effect, and a note when any effect may need a particular subclass
/// or ability.
fn draw_sources(
    ui: &mut egui::Ui,
    perks: &Perks,
    members: &[&WeaponSandboxPerkChoice],
    first: &WeaponSandboxPerkChoice,
) {
    let Some(ingredients) = perks.ingredients else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        if let Some(sources) = ingredients.sources.get(&first.perk_index) {
            let heading = if sources.len() == 1 {
                "Source"
            } else {
                "Sources"
            };
            ui.label(format!(
                "{heading}: {}",
                sources
                    .iter()
                    .map(|source| source.label())
                    .collect::<Vec<_>>()
                    .join(" and ")
            ));
        }
        if members.iter().any(|choice| {
            ingredients
                .context
                .get(&choice.perk_index)
                .is_some_and(|context| !context.is_empty())
        }) {
            sundial::investment::draw_authoring_info_icon(
                ui,
                "May need a specific subclass or ability.",
            );
        }
    });
}
