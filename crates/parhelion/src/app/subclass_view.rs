//! The main page for subclasses. A subclass keeps its base's element or shows another damage type
//! icon, and is for every class, its base's or another. Each ability and each attunement path node is based on a stock one of any
//! class, and can be authored: a name, a description, an icon and perks of its own, including
//! custom perks from the perk workbench. An attunement can come from another stock subclass and
//! take its own name.
//!
//! The page lists the subclass at the left in the game's order: its abilities by slot, then its
//! attunements with their nodes. Clicking any of them shows it at the right: an ability or node
//! heads with its icon, its name and the stock one it is based on, whose choices open over the
//! page, then its sections as tabs: Ability (name, description, icon), Perks (the perks it
//! grants), Gameplay (its Ability card of charges, recharge, parameters and its own values, a
//! node's Ability Changes, what it spawns, and a closed Technical section with raw values) and
//! Visuals.
use super::*;
use crate::app::style;
use crate::subclass::{
    AbilitySlot, AttunementPath, EntryEdits, EntryIcon, MOST_CHARGES, Place, SubclassAbilities,
    SubclassPathNode, layout,
};
use sundial::investment::{DisplayTooltip, SubclassSummary, draw_display_tooltip};

mod appearance;
mod attached;
mod choices;
mod colors;
mod detail;
mod generated_icon;
mod hud;
mod icon;
mod list;
mod modifiers;
mod perks;
mod properties;
mod tuning;
mod values;

pub(in crate::app) use properties::GraphCards;

/// The list's width beside the detail panel, and the narrowest pane that keeps the two side by
/// side.
const LIST_WIDTH: f32 = 360.0;
const SIDE_BY_SIDE_WIDTH: f32 = 860.0;
/// The icons in choices and chips, and in the detail panel's heading.
const CHOICE_ICON: f32 = 18.0;
const HEADING_ICON: f32 = 48.0;
/// The detail panel's field labels, and the column that names each choice's subclass.
const LABEL_WIDTH: f32 = 110.0;

/// Attunements in the order the game shows them, the middle one between the others.
const DISPLAY_PATHS: [AttunementPath; 3] = [
    AttunementPath::Top,
    AttunementPath::Middle,
    AttunementPath::Bottom,
];

/// What the detail panel shows: an ability or node, or an attunement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubclassSelection {
    Entry(Place),
    Path(AttunementPath),
}

impl Default for SubclassSelection {
    fn default() -> Self {
        Self::Entry(Place::Ability(layout::CLASS_ABILITIES[0]))
    }
}

/// The subclass page's own state.
pub(super) struct PageState {
    pub(super) selection: SubclassSelection,
    /// The search of the detail panel's choices.
    search: String,
    /// The selection whose Based On choices are open below its row.
    pub(super) choosing: Option<SubclassSelection>,
    /// The attunement the list shows.
    path_tab: AttunementPath,
    /// Each node's icon browser is independent of the generated inventory icon's symbol.
    icons: icon::Picker,
    attached: attached::Abilities,
    /// The generated inventory icon's symbol.
    artwork: crate::artwork_browser::Picker,
    artwork_query: String,
    /// The generated icon's symbol from a stock perk, until its texture is read.
    symbol: Option<Receiver<Result<crate::perk::Icon, String>>>,
    /// The abilities' entities and the list of their values.
    values: values::Values,
    /// Each ability's tree of graphs and the properties they hold.
    properties: properties::Properties,
    /// The open section of an ability or node, kept as the selection moves.
    section: detail::Section,
    /// The Parameters card's filter and the Raw Values trail.
    parameter_query: String,
    property_query: String,
    trail: tuning::Trail,
    /// The modifier Add Change is putting together.
    modifier_draft: Option<modifiers::Draft>,
    /// The palettes each ability's effects draw with, and their swatches.
    colors: colors::Colors,
    /// The Appearance page's Screen Art.
    art: appearance::ArtPage,
}

impl Default for PageState {
    fn default() -> Self {
        Self {
            selection: SubclassSelection::default(),
            search: String::new(),
            choosing: None,
            path_tab: AttunementPath::Top,
            icons: icon::Picker::default(),
            attached: attached::Abilities::default(),
            artwork: crate::artwork_browser::Picker::for_purpose(
                crate::artwork_browser::Purpose::Perk,
            ),
            artwork_query: String::new(),
            symbol: None,
            values: values::Values::default(),
            properties: properties::Properties::default(),
            section: detail::Section::default(),
            parameter_query: String::new(),
            property_query: String::new(),
            trail: tuning::Trail::default(),
            modifier_draft: None,
            colors: colors::Colors::default(),
            art: appearance::ArtPage::default(),
        }
    }
}

impl PageState {
    /// Whether the Add Modifier form is open.
    #[cfg(test)]
    pub(super) const fn adding_modifier(&self) -> bool {
        self.modifier_draft.is_some()
    }
}

/// One change the page makes to the recipe's abilities.
enum AbilityEdit {
    /// Bases the ability in an entry on another stock ability, keeping its edits.
    AbilitySource(u8, (u32, u8)),
    ResetAbility(u8),
    /// Restores the base's own attunement, nodes and name.
    ResetAttunement(AttunementPath),
    PathSource(AttunementPath, (u32, AttunementPath)),
    PathName(AttunementPath, Option<String>),
    /// Bases a node on another stock node, keeping its edits.
    NodeSource(AttunementPath, u8, (u32, AttunementPath, u8)),
    ResetNode(AttunementPath, u8),
    Edits(Place, Box<EntryEdits>),
    /// Opens a custom perk of an ability or node in the workbench.
    OpenPerk(Place, custom_perks::workbench::AbilityPerk),
    Restore,
}

/// The stock ability an entry is based on: its subclass and its entry there.
fn source_of(abilities: &SubclassAbilities, base: u32, place: Place) -> (u32, u8) {
    match place {
        Place::Ability(entry) => {
            let choice = abilities.ability(base, entry);
            (choice.source, choice.source_entry)
        }
        Place::Node(path, position) => {
            let node = abilities.node(base, path, position);
            (node.source, node.source_entry())
        }
    }
}

/// The stock ability an entry holds when the recipe leaves it alone.
fn own_source(abilities: &SubclassAbilities, base: u32, place: Place) -> (u32, u8) {
    match place {
        Place::Ability(entry) => (base, entry),
        Place::Node(path, position) => {
            let (source, source_path) = abilities.attunement_source(base, path);
            (source, source_path.entries()[usize::from(position)])
        }
    }
}

/// Whether the recipe changes the entry at `place` from what its path or base holds.
fn is_own(abilities: &SubclassAbilities, place: Place) -> bool {
    match place {
        Place::Ability(entry) => abilities.choice(entry).is_some(),
        Place::Node(path, position) => abilities
            .attunement(path)
            .is_some_and(|attunement| attunement.node(position).is_some()),
    }
}

fn find_subclass(subclasses: &[SubclassSummary], hash: u32) -> Option<&SubclassSummary> {
    subclasses.iter().find(|subclass| subclass.hash == hash)
}

fn entry_name(subclass: &SubclassSummary, entry: u8) -> &str {
    if entry == layout::BASE_MOVEMENT {
        return "Base Movement";
    }
    if entry == layout::STAT_PASSIVES {
        return "Stat Passives";
    }
    subclass
        .entry_names
        .get(&entry)
        .map_or("Unknown Ability", String::as_str)
}

fn attunement_name(subclass: &SubclassSummary, path: AttunementPath) -> &str {
    subclass
        .attunement_names
        .get(path.index())
        .map_or(path.label(), String::as_str)
}

/// Where something comes from, when that is another subclass than the base: its name and class.
fn from_label(subclass: Option<&SubclassSummary>, base: u32) -> Option<String> {
    let subclass = subclass.filter(|subclass| subclass.hash != base)?;
    Some(match gear_view::class_label(subclass.class_type) {
        Some(class) => format!("From {} · {class}", subclass.name),
        None => format!("From {}", subclass.name),
    })
}

/// A tooltip's line under a name: what it is, and the stock subclass it comes from.
fn from_subtitle(kind: &str, subclass: Option<&SubclassSummary>) -> String {
    match subclass {
        Some(subclass) => format!("{kind} · {}", subclass.name),
        None => kind.to_owned(),
    }
}

/// Text on one line, cut with an ellipsis where it would run past `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Small, quiet text: a section's name, a field's label, a subclass beside its choices.
fn quiet(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(12.0)
        .color(style::secondary(ui.visuals()))
}

/// Paints a texture into `rect`.
fn paint_icon(ui: &egui::Ui, icon: &egui::TextureHandle, rect: egui::Rect) {
    ui.painter().image(
        icon.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

impl PackageAuthoringApp {
    pub(super) fn draw_subclass_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        if let Some(base_width) = workbench_left_column_width(ui.available_width()) {
            let definition_width = ui.available_width() - base_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(base_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(base_width);
                        self.draw_gear_base(ui);
                        ui.add_space(8.0);
                        self.draw_icon_donor_picker(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(definition_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(definition_width);
                        self.draw_subclass_definition(ui);
                    },
                );
            });
        } else {
            self.draw_gear_base(ui);
            ui.add_space(8.0);
            self.draw_icon_donor_picker(ui);
            ui.separator();
            self.draw_subclass_definition(ui);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        self.draw_subclass_abilities(ui);
    }

    /// The subclass's text, then its Class and Damage Type Icon pickers under it, in the first
    /// column as gear pages place theirs.
    fn draw_subclass_definition(&mut self, ui: &mut egui::Ui) {
        self.draw_item_text(ui, Some(self.subclass_type_name()));
        ui.add_space(4.0);
        style::tiles(ui, |ui, width| {
            style::tile_column(ui, (width, "subclass-class"), |ui| {
                self.draw_subclass_class(ui)
            });
            style::tile_column(ui, (width, "subclass-damage-type"), |ui| {
                self.draw_subclass_damage_type(ui)
            });
            style::tile_column(ui, (width, "subclass-hud"), |ui| self.draw_subclass_hud(ui));
        });
    }

    /// The damage type whose icon the subclass shows beside its name: its base's, or another.
    /// It changes no ability's damage.
    fn draw_subclass_damage_type(&mut self, ui: &mut egui::Ui) {
        use crate::recipe::RecipeDamageType;
        use sundial::investment::WeaponDamageType;
        const TYPES: [(RecipeDamageType, &str); 4] = [
            (RecipeDamageType::Arc, "Arc"),
            (RecipeDamageType::Solar, "Solar"),
            (RecipeDamageType::Void, "Void"),
            (RecipeDamageType::Kinetic, "Kinetic"),
        ];
        let base =
            self.subclass_base()
                .and_then(|base| base.damage_type)
                .map(|damage| match damage {
                    WeaponDamageType::Kinetic => RecipeDamageType::Kinetic,
                    WeaponDamageType::Arc => RecipeDamageType::Arc,
                    WeaponDamageType::Solar => RecipeDamageType::Solar,
                    WeaponDamageType::Void => RecipeDamageType::Void,
                });
        let label = |damage: RecipeDamageType| {
            TYPES
                .iter()
                .find(|(each, _)| *each == damage)
                .map_or("", |(_, label)| *label)
        };
        let base_label = base.map_or("Base Damage Type", label);
        let current = self.recipe.overrides.subclass_damage_type;
        let (name, reset) = style::stock_field_name(
            ui,
            "Damage Type Icon",
            "The icon beside its name, not its damage",
            current.map(|_| base_label),
        );
        let mut choice = if reset { None } else { current };
        egui::ComboBox::from_id_salt("subclass_damage_type")
            .selected_text(current.map_or(base_label, label))
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                workbench_style(ui);
                ui.selectable_value(&mut choice, None, base_label);
                for (damage, label) in TYPES {
                    if Some(damage) != base {
                        ui.selectable_value(&mut choice, Some(damage), label);
                    }
                }
            })
            .response
            .labelled_by(name.id);
        if choice != current {
            self.recipe.overrides.subclass_damage_type = choice;
        }
    }

    /// The type label the subclass's text defaults to, by the classes it is for.
    fn subclass_type_name(&self) -> &'static str {
        let overrides = &self.recipe.overrides;
        crate::subclass::class_type_name(overrides.subclass_every_class, overrides.subclass_class)
            .unwrap_or("Subclass")
    }

    /// Which classes the subclass is for, as armor's Class picker chooses: Any Class, which
    /// every character receives and may equip, its base's class, or another class, whose
    /// requirement then names that class and whose characters receive it.
    fn draw_subclass_class(&mut self, ui: &mut egui::Ui) {
        use crate::ArmorClass;
        let base = self.subclass_base().map(|base| base.class_type);
        let overrides = &mut self.recipe.overrides;
        let current = if overrides.subclass_every_class {
            Some(ArmorClass::Any)
        } else {
            overrides.subclass_class
        };
        let base_label = base
            .and_then(gear_view::class_label)
            .unwrap_or("Base Class");
        let (label, reset) = style::stock_field_name(
            ui,
            "Class",
            "Which characters receive and can equip it",
            current.map(|_| base_label),
        );
        let mut choice = if reset { None } else { current };
        egui::ComboBox::from_id_salt("subclass_class")
            .selected_text(current.map_or(base_label, ArmorClass::label))
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                workbench_style(ui);
                ui.selectable_value(&mut choice, Some(ArmorClass::Any), ArmorClass::Any.label());
                ui.selectable_value(&mut choice, None, base_label);
                for class in [ArmorClass::Titan, ArmorClass::Hunter, ArmorClass::Warlock] {
                    if class.native_class() != base {
                        ui.selectable_value(&mut choice, Some(class), class.label());
                    }
                }
            })
            .response
            .labelled_by(label.id);
        if choice != current {
            overrides.subclass_every_class = choice == Some(ArmorClass::Any);
            overrides.subclass_class = choice.filter(|class| *class != ArmorClass::Any);
        }
    }

    /// The base subclass the open recipe builds on, once the catalog has it.
    fn subclass_base(&self) -> Option<SubclassSummary> {
        self.recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .and_then(|hash| find_subclass(&self.subclasses, hash))
            .cloned()
    }

    fn draw_subclass_abilities(&mut self, ui: &mut egui::Ui) {
        let Some(base) = self.subclass_base() else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Base subclass not found in the catalog.",
            );
            return;
        };
        sundial::ui::notice(
            ui,
            "Ability Authoring Is Experimental",
            "Some changes may not behave as expected in game, and some values are not fully \
             understood yet. Ability authoring will improve in future releases.",
        );
        self.subclass_page
            .icons
            .poll(&mut self.recipe, base.hash, ui.ctx());
        let abilities = self
            .recipe
            .overrides
            .subclass_abilities
            .clone()
            .unwrap_or_default();
        // The Projectile browser's All Projectiles list, and a swap to a projectile no stock ability
        // fires, read the engine catalog the perk workbench loads.
        if self.subclass_page.properties.wants_catalog
            && self.build_receiver.is_none()
            && self.install_receiver.is_none()
        {
            self.perk_workbench
                .prepare_assets(ui.ctx(), &self.packages, self.catalog.as_ref());
        }
        let mut page = std::mem::take(&mut self.subclass_page);
        page.artwork.poll();
        if page.artwork.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let ((clicked, restore), edit) = if ui.available_width() >= SIDE_BY_SIDE_WIDTH {
            ui.horizontal_top(|ui| {
                let clicked = ui
                    .allocate_ui_with_layout(
                        egui::vec2(LIST_WIDTH, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(LIST_WIDTH);
                            self.draw_subclass_list(ui, &base, &abilities, &page)
                        },
                    )
                    .inner;
                let width = ui.available_width();
                let edit = ui
                    .allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            self.draw_subclass_detail(ui, &base, &abilities, &mut page)
                        },
                    )
                    .inner;
                (clicked, edit)
            })
            .inner
        } else {
            let clicked = self.draw_subclass_list(ui, &base, &abilities, &page);
            ui.add_space(8.0);
            let edit = self.draw_subclass_detail(ui, &base, &abilities, &mut page);
            (clicked, edit)
        };
        let edit = if restore {
            Some(AbilityEdit::Restore)
        } else {
            edit
        };
        if let Some(selection) = clicked
            && selection != page.selection
        {
            page.selection = selection;
            page.search.clear();
            page.parameter_query.clear();
            page.property_query.clear();
            // An open form belongs to the row it was opened on.
            page.modifier_draft = None;
        }
        // The attunement tabs follow the selection.
        if let SubclassSelection::Path(path) | SubclassSelection::Entry(Place::Node(path, _)) =
            page.selection
        {
            page.path_tab = path;
        }
        self.subclass_page = page;
        if let Some(edit) = edit {
            self.apply_ability_edit(base.hash, abilities, edit);
        }
    }

    /// Keep graph and bank edits when two choices share an entity. Pool removals belong to the
    /// source choice. Controls without a compatible new ability must lose their hidden edits.
    fn keep_own_values(&self, edits: &mut EntryEdits, from: (u32, u8), to: (u32, u8)) {
        let entity_of = |(source, entry): (u32, u8)| {
            find_subclass(&self.subclasses, source)
                .and_then(|subclass| subclass.entry_entities.get(&entry).copied())
        };
        let entity = entity_of(to);
        if from != to {
            edits.removed_modifiers.clear();
        }
        if entity_of(from) != entity {
            edits.ability_values.clear();
            edits.parameters.clear();
            edits.bank_values.clear();
            edits.palettes.clear();
            edits.tints.clear();
            edits.spawn_swaps.clear();
            edits.attached_abilities.clear();
        }
        let row = find_subclass(&self.subclasses, to.0)
            .and_then(|summary| summary.entry_rows.get(&to.1))
            .and_then(|row| self.catalog.as_ref()?.ability_row(*row));
        if !row.is_some_and(|row| row.charges) {
            edits.extra_charges = 0;
        }
        if !row.is_some_and(|row| row.recharge) {
            edits.set_recharge(None);
        }
        if entity.is_none() {
            edits.grade = None;
            edits.damage_type = None;
        }
    }

    fn apply_ability_edit(
        &mut self,
        base: u32,
        mut abilities: SubclassAbilities,
        edit: AbilityEdit,
    ) {
        if matches!(
            edit,
            AbilityEdit::ResetAbility(_)
                | AbilityEdit::ResetAttunement(_)
                | AbilityEdit::ResetNode(..)
                | AbilityEdit::Restore
        ) {
            self.subclass_page.icons.cancel();
        }
        match edit {
            AbilityEdit::AbilitySource(entry, (source, source_entry)) => {
                let old = abilities.ability(base, entry);
                let from = (old.source, old.source_entry);
                let mut choice = crate::subclass::SubclassChoice {
                    source,
                    source_entry,
                    ..old
                };
                self.keep_own_values(&mut choice.edits, from, (source, source_entry));
                abilities.set_ability(base, choice);
            }
            AbilityEdit::ResetAbility(entry) => abilities.reset_ability(entry),
            AbilityEdit::ResetAttunement(path) => abilities.reset_attunement(path),
            AbilityEdit::PathSource(path, source) => abilities.set_path_source(path, base, source),
            AbilityEdit::PathName(path, name) => abilities.set_path_name(path, base, name),
            AbilityEdit::NodeSource(path, position, (source, source_path, source_position)) => {
                let old = abilities.node(base, path, position);
                let from = (old.source, old.source_entry());
                let mut node = SubclassPathNode {
                    source,
                    source_path,
                    source_position,
                    ..old
                };
                let source_entry = node.source_entry();
                self.keep_own_values(&mut node.edits, from, (source, source_entry));
                abilities.set_path_node(path, base, node);
            }
            AbilityEdit::ResetNode(path, position) => {
                let (source, source_path) = abilities.attunement_source(base, path);
                let own = SubclassPathNode::stock(position, source, source_path, position);
                abilities.set_path_node(path, base, own);
            }
            AbilityEdit::Edits(place, edits) => abilities.set_edits(base, place, *edits),
            AbilityEdit::OpenPerk(place, perk) => {
                self.perk_request = Some(custom_perks::workbench::Request::Ability { place, perk });
                return;
            }
            AbilityEdit::Restore => abilities = SubclassAbilities::default(),
        }
        self.recipe.overrides.subclass_abilities = (!abilities.is_empty()).then_some(abilities);
    }

    /// Every ability and node of the open subclass recipe with its name, which a custom perk can
    /// go on. Empty for other recipes.
    pub(super) fn subclass_places(&self) -> Vec<(Place, String)> {
        if self.recipe.kind != ItemKind::Subclass {
            return Vec::new();
        }
        let Some(base) = self.subclass_base() else {
            return Vec::new();
        };
        let abilities = self
            .recipe
            .overrides
            .subclass_abilities
            .clone()
            .unwrap_or_default();
        Place::editable()
            .map(|place| {
                let name = self.entry_title(&abilities, base.hash, place);
                (place, format!("{} · {name}", place.label()))
            })
            .collect()
    }

    /// An ability's or node's name: its own, or the stock one it is based on.
    fn entry_title(&self, abilities: &SubclassAbilities, base: u32, place: Place) -> String {
        abilities.edits(base, place).name.unwrap_or_else(|| {
            let (source, entry) = source_of(abilities, base, place);
            find_subclass(&self.subclasses, source).map_or_else(
                || "Unknown Ability".to_owned(),
                |subclass| entry_name(subclass, entry).to_owned(),
            )
        })
    }

    /// What a stock subclass's entry does: its node's own description, or else the first of its
    /// perks that says.
    fn entry_description(&self, subclass: Option<&SubclassSummary>, entry: u8) -> Option<String> {
        let subclass = subclass?;
        if let Some(description) = subclass.entry_descriptions.get(&entry) {
            return Some(description.clone());
        }
        let catalog = self.catalog.as_ref()?;
        subclass
            .entry_perks
            .get(&entry)?
            .iter()
            .filter_map(|perk| catalog.perk_component_description(*perk))
            .find(|text| !text.trim().is_empty())
            .map(str::to_owned)
    }

    /// The icon a stock subclass's entry shows, once it has loaded.
    fn entry_icon(
        &self,
        ctx: &egui::Context,
        subclass: Option<&SubclassSummary>,
        entry: u8,
    ) -> Option<egui::TextureHandle> {
        let container = *subclass?.entry_icons.get(&entry)?;
        self.catalog.as_ref()?.subclass_icon(ctx, container)
    }

    /// The icon an ability or node shows: its own, or its source's.
    fn place_icon(
        &self,
        ctx: &egui::Context,
        abilities: &SubclassAbilities,
        base: u32,
        place: Place,
    ) -> Option<egui::TextureHandle> {
        match abilities.edits(base, place).icon {
            Some(EntryIcon::Ability { subclass, entry }) => {
                self.entry_icon(ctx, find_subclass(&self.subclasses, subclass), entry)
            }
            Some(EntryIcon::Artwork { artwork }) => {
                crate::artwork_browser::preview::texture(ctx, self.catalog.as_ref()?, &artwork)
            }
            None => {
                let (source, entry) = source_of(abilities, base, place);
                self.entry_icon(ctx, find_subclass(&self.subclasses, source), entry)
            }
        }
    }
}
