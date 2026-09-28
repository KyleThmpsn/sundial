//! The asset picker shared by spawn, attach and pattern actions, and the native fields an
//! attach node carries beside its asset.
use super::*;
use sundial::package_authoring::sandbox_perk::program::Asset;

pub(super) mod suggested;
pub(super) mod variants;

/// Which catalog entries an action may reference.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetScope {
    /// A pattern override needs a projectile.
    Projectiles,
    /// A spawn accepts a projectile, emitter, pickup or physical world object.
    Spawnable,
    /// An attach accepts any entity graph of a type stock perks attach.
    Any,
    /// An ammo drop's optional effect, shown on the drop. Stock drops use emitters.
    DropEffect,
}

/// A technical name without the tag it ends in, for a list row.
fn without_tag(name: &str) -> &str {
    match name.rsplit_once(" · 0x") {
        Some((rest, tag)) if tag.len() == 8 && tag.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            rest
        }
        _ => name,
    }
}

impl AssetScope {
    fn placement_hint(self, kind: projectile::Kind) -> &'static str {
        match self {
            Self::Projectiles => "Fired by the weapon in place of its current projectile.",
            Self::Spawnable if kind == projectile::Kind::Projectile => {
                "Starts at Spawn Location and uses the asset's native motion."
            }
            Self::Spawnable => "Created at Spawn Location.",
            Self::Any => {
                "Attached to the selected object. Lifetime depends on its cleanup settings."
            }
            Self::DropEffect => "Shown on the ammo this action drops. Optional.",
        }
    }

    fn picker_labels(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Projectiles => (
                "Choose Projectile…",
                "Choose a Projectile",
                "Use Projectile",
            ),
            Self::Spawnable => (
                "Choose Object or Effect…",
                "Choose an Object or Effect to Spawn",
                "Use for Spawn",
            ),
            Self::Any => (
                "Choose Attachment…",
                "Choose an Attachment",
                "Use Attachment",
            ),
            Self::DropEffect => (
                "Choose Drop Effect…",
                "Choose a Drop Effect",
                "Use Drop Effect",
            ),
        }
    }

    fn allows(self, kind: projectile::Kind, object_type: u8) -> bool {
        match self {
            Self::Projectiles => kind == projectile::Kind::Projectile,
            Self::Spawnable => projectile::spawnable_object(kind, object_type),
            Self::Any | Self::DropEffect => true,
        }
    }
}

/// How the asset results are ordered. Order changes presentation only. It never hides a
/// result and never changes which assets an action accepts.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Order {
    /// Query matches first, then the assets a player knows by name in the order
    /// [`suggested`] gives, then what the most carried stock perks reference, then assets the
    /// packages name directly.
    #[default]
    Suggested,
    Name,
    Kind,
    Source,
}

impl Order {
    const ALL: [Self; 4] = [Self::Suggested, Self::Name, Self::Kind, Self::Source];

    fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Name => "Name",
            Self::Kind => "Type",
            Self::Source => "Package",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Suggested => "Search matches, then well-known assets, then the most used.",
            Self::Name => "Every result by name, including unnamed assets by tag.",
            Self::Kind => "Grouped by object type, then by name.",
            Self::Source => "Grouped by the installed package the asset lives in.",
        }
    }

    fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
}

/// The comparable key for one result. Every order ends with the name, so results never
/// reshuffle between frames.
fn asset_sort_key(
    order: Order,
    entry: &projectile::catalog::Entry,
    asset: &AssetSearch,
    direct_match: bool,
) -> (bool, usize, std::cmp::Reverse<usize>, u8, String, String) {
    let name = asset.name.clone();
    let none = std::cmp::Reverse(0);
    match order {
        Order::Suggested => (
            !direct_match,
            asset.lead,
            std::cmp::Reverse(asset.uses),
            entry.label_rank(),
            String::new(),
            name,
        ),
        Order::Name => (false, 0, none, 0, String::new(), name),
        Order::Kind => (false, 0, none, 0, entry.kind_label().to_lowercase(), name),
        Order::Source => (false, 0, none, 0, entry.source_label().to_lowercase(), name),
    }
}

fn asset_usage(
    entry: &projectile::catalog::Entry,
    perk_names: &BTreeMap<u16, String>,
    discovery: &discovery::Discovery,
) -> String {
    let mut uses = BTreeSet::new();
    for index in entry
        .perk_indices
        .iter()
        .copied()
        .chain(entry.contexts.iter().filter_map(|context| context.perk))
    {
        if let Some(name) = perk_names.get(&index) {
            let behavior = discovery
                .behavior(index)
                .map(|behavior| behavior.headline.as_str())
                .unwrap_or_default();
            uses.insert(format!("Referenced by {name}: {behavior}"));
        }
    }
    if uses.len() > 1 {
        entry.source_hint.clone().unwrap_or_else(|| {
            "Shared by multiple perks. No common behavior has been established.".into()
        })
    } else {
        uses.into_iter().collect::<Vec<_>>().join("\n")
    }
}

impl Workbench {
    /// An attachment with no name of its own is named after this effect. When the picker's
    /// name already names this perk, as "Firefly Attachment" or "Attachment Shared by Ace of
    /// Spades Catalyst, Firefly" do, the list uses that same name, so what a user
    /// reads here is what they can search for. A name that comes from an ancestor
    /// instead, which is not the entity's role in this effect, gives way to this effect's
    /// own numbering.
    pub(super) fn program_asset_labels(
        &self,
        program: &sundial::package_authoring::sandbox_perk::program::NativeProgram,
        name: &str,
    ) -> BTreeMap<u32, String> {
        let attachments: BTreeSet<_> = program
            .graph
            .blocks
            .iter()
            .filter(|block| matches!(block.class, 0x80803E45 | 0x80803E44))
            .filter_map(|block| block.bytes.get(16..20))
            .filter_map(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
            .collect();
        let context = name.split(" · Effect ").next().unwrap_or(name).trim();
        program
            .assets
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                let label = self
                    .asset_labels
                    .get(&asset.graph)
                    .cloned()
                    .unwrap_or_else(|| format!("Asset 0x{:08X}", asset.graph));
                let shared_attachment = attachments.contains(&asset.graph)
                    && self.discovery.data.as_ref().is_some_and(|data| {
                        data.effects
                            .entries
                            .iter()
                            .find(|entry| entry.graph == asset.graph)
                            .is_some_and(|entry| {
                                entry.kind == projectile::Kind::Entity
                                    && entry.direct_name().is_none()
                            })
                    });
                (
                    asset.graph,
                    if shared_attachment && !label.contains(context) {
                        format!("{context} Attachment {}", index + 1)
                    } else {
                        label
                    },
                )
            })
            .collect()
    }

    pub(super) fn draw_asset_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        asset: &mut Asset,
        scope: AssetScope,
    ) {
        self.refresh_asset_labels();
        let (empty_label, title, use_label) = scope.picker_labels();
        let label = asset_label(asset, &self.asset_labels, empty_label);
        // Optional references such as a drop's effect store all bits set for none.
        let current = Some(asset.graph).filter(|graph| !matches!(*graph, 0 | u32::MAX));
        let picked = pickers::browser_with_toolbar(
            ui,
            "program-asset",
            &label,
            title,
            &mut self.asset_query,
            |ui, query, reset, _height| {
                Browser {
                    catalog,
                    discovery: &self.discovery,
                    perk_names: &self.perk_names,
                    item_names: &self.item_names,
                    asset_labels: &self.asset_labels,
                    // The picker chooses an effect; markers are inspection, not selection.
                    markers: None,
                    carried: self
                        .ingredients
                        .as_ref()
                        .map(|(_, _, ingredients)| &ingredients.sources),
                }
                .draw(ui, scope, query, reset, Some(use_label), current)
            },
        );
        if let Some(picked) = picked
            && picked.graph != asset.graph
        {
            *asset = picked;
        }
    }
}

/// A saved native path remains useful while discovery is loading or lacks ancestry.
pub(super) fn asset_label(asset: &Asset, labels: &BTreeMap<u32, String>, empty: &str) -> String {
    labels
        .get(&asset.graph)
        .filter(|name| !name.starts_with("Unidentified "))
        .cloned()
        .or_else(|| {
            (!asset.path.is_empty())
                .then(|| sundial::package_authoring::tft::asset_label(&asset.path))
        })
        .unwrap_or_else(|| {
            // Optional references such as a drop's effect store all bits set for none.
            if matches!(asset.graph, 0 | u32::MAX) {
                empty.into()
            } else {
                labels
                    .get(&asset.graph)
                    .cloned()
                    .unwrap_or_else(|| format!("Asset 0x{:08X}", asset.graph))
            }
        })
}

/// Shared asset list, filters and details for selection and Engine Catalog inspection.
pub(super) struct Browser<'a> {
    pub catalog: Option<&'a InvestmentCatalog>,
    pub discovery: &'a discovery::Discovery,
    pub perk_names: &'a BTreeMap<u16, String>,
    pub item_names: &'a BTreeMap<u32, projectile::catalog::ItemName>,
    pub asset_labels: &'a BTreeMap<u32, String>,
    /// The marker index, when the Markers view has read it. An object's attachment points are
    /// part of what it is, so they belong on its page and not only in the marker list.
    pub markers: Option<&'a sundial::package_authoring::gear_markers::MarkerIndex>,
    /// What carries each stock perk, once the installation is read, so Suggested counts only
    /// the perks weapons, armor and abilities carry.
    pub carried: Option<&'a BTreeMap<u16, BTreeSet<sundial::investment::IngredientSource>>>,
}

/// The result set for one (query, scope, filter, order, visibility, search index)
/// combination, with how many results the default visibility hides when the set is empty.
type ResultCache = std::sync::Arc<(
    (String, u8, u8, u8, bool, usize),
    Vec<usize>,
    usize,
    Vec<variants::Group>,
)>;

/// Everything a search can match for one asset, lowercased once. Typing then scans these
/// strings and allocates nothing.
struct AssetSearch {
    /// The asset's lowercase label, for ordering and direct-match detection.
    name: String,
    /// The asset's own text: label, catalog search text, source hint and item names.
    own: String,
    /// Perks whose shared text in [`SearchIndex::perks`] also answers for this asset. Stored
    /// as indices so a perk's behavior text is held once, not once per asset that uses it.
    perks: Vec<u16>,
    /// Whether the asset has a discovery identity, which the default visibility requires.
    identified: bool,
    /// The game's name for an asset its label names by a native name, such as Hammer of Sol.
    game: Option<String>,
    /// Where Suggested places the asset among the ones a player knows by name.
    lead: usize,
    /// How many carried stock perks reference the asset, which Suggested lists most of first.
    uses: usize,
}

/// One search index per catalog and label set, keyed by their sizes and identity.
struct SearchIndex {
    key: (usize, usize, usize, usize, usize, usize),
    assets: Vec<AssetSearch>,
    unidentified: usize,
    /// Lowercase name and behavior text per referenced perk, shared by every asset using it.
    perks: BTreeMap<u16, String>,
}

impl SearchIndex {
    fn matches(&self, asset: &AssetSearch, word: &str) -> bool {
        asset.own.contains(word)
            || asset
                .perks
                .iter()
                .any(|perk| self.perks.get(perk).is_some_and(|text| text.contains(word)))
    }
}

impl Browser<'_> {
    /// The prepared search text for this catalog, kept until the catalog or its names change.
    fn search_index(&self, ui: &egui::Ui, data: &discovery::Data) -> std::sync::Arc<SearchIndex> {
        let key = (
            std::sync::Arc::as_ptr(&data.perks) as usize,
            data.asset_choices.len(),
            self.asset_labels.len(),
            self.perk_names.len(),
            self.item_names.len(),
            self.carried
                .map_or(0, |carried| std::ptr::from_ref(carried) as usize),
        );
        let cache_id = ui.make_persistent_id("asset-search-index");
        if let Some(index) = ui
            .data(|state| state.get_temp::<std::sync::Arc<SearchIndex>>(cache_id))
            .filter(|index| index.key == key)
        {
            return index;
        }
        let index = std::sync::Arc::new(build_search_index(
            data,
            key,
            self.asset_labels,
            self.perk_names,
            self.item_names,
            self.carried,
        ));
        ui.data_mut(|state| state.insert_temp(cache_id, index.clone()));
        index
    }
}

/// Lowercases everything a search can match, once per catalog.
///
/// Typing then scans prepared strings and allocates nothing, which is what keeps a six thousand
/// asset list responsive. Perk text is held once per perk rather than once per asset that
/// references it, because a single perk can name hundreds of them. Pure, so the text a search
/// actually sees is tested without drawing a frame.
///
/// Suggested's inputs are read here too, once per asset: its place among the assets a player
/// knows by name, whose game name search also answers to, and how many carried stock perks
/// reference it.
fn build_search_index(
    data: &discovery::Data,
    key: (usize, usize, usize, usize, usize, usize),
    asset_labels: &BTreeMap<u32, String>,
    perk_names: &BTreeMap<u16, String>,
    item_names: &BTreeMap<u32, projectile::catalog::ItemName>,
    carried: Option<&BTreeMap<u16, BTreeSet<sundial::investment::IngredientSource>>>,
) -> SearchIndex {
    let mut perks = BTreeMap::new();
    let assets: Vec<AssetSearch> = data
        .asset_choices
        .iter()
        .map(|row| {
            let entry = &data.effects.entries[row.index];
            let name = asset_labels
                .get(&entry.graph)
                .map(|name| name.to_lowercase())
                .unwrap_or_default();
            let mut own = row.search.to_lowercase();
            let mut push = |text: &str| {
                own.push(' ');
                own.push_str(text);
            };
            push(&name);
            let game = suggested::game_name(&name);
            if let Some(game) = &game {
                push(&game.to_lowercase());
            }
            if let Some(hint) = &entry.source_hint {
                push(&hint.to_lowercase());
            }
            for item in entry.contexts.iter().filter_map(|context| context.item) {
                if let Some(item) = item_names.get(&item) {
                    push(&item.name.to_lowercase());
                }
            }
            let mut referenced = entry
                .perk_indices
                .iter()
                .copied()
                .chain(entry.contexts.iter().filter_map(|context| context.perk))
                .collect::<Vec<_>>();
            referenced.sort_unstable();
            referenced.dedup();
            for perk in &referenced {
                perks.entry(*perk).or_insert_with(|| {
                    let mut text = perk_names
                        .get(perk)
                        .map(|name| name.to_lowercase())
                        .unwrap_or_default();
                    if let Some(behavior) = data
                        .perks
                        .perks
                        .get(usize::from(*perk))
                        .filter(|entry| entry.index == usize::from(*perk))
                        .and_then(|entry| entry.behavior.as_ref())
                    {
                        text.push(' ');
                        text.push_str(&guidance::behavior_search(behavior).to_lowercase());
                    }
                    text
                });
            }
            let identified = entry.has_discovery_identity_with(
                |index| perk_names.get(&index).cloned(),
                |item| item_names.get(&item).cloned(),
            );
            let uses = referenced
                .iter()
                .filter(|perk| behaviors::counts(carried, **perk))
                .count();
            AssetSearch {
                lead: suggested::rank(&name),
                name,
                own,
                perks: referenced,
                identified,
                game,
                uses,
            }
        })
        .collect();
    let unidentified = assets
        .iter()
        .filter(|asset: &&AssetSearch| !asset.identified)
        .count();
    SearchIndex {
        key,
        assets,
        unidentified,
        perks,
    }
}

/// What the browser's controls select, apart from the query itself.
#[derive(Clone, Copy)]
struct Selection {
    scope: AssetScope,
    /// The type combo's value, where zero is every type. Ignored by the projectile picker,
    /// which is already limited to one type.
    filter: u8,
    order: Order,
    /// Whether assets the packages never name are listed as well.
    show_all: bool,
}

impl Selection {
    fn allows(self, entry: &projectile::catalog::Entry) -> bool {
        if !self.scope.allows(entry.kind, entry.object_type) {
            return false;
        }
        if self.scope == AssetScope::Projectiles {
            return true;
        }
        match self.filter {
            1 => entry.kind == projectile::Kind::Projectile,
            2 => entry.kind == projectile::Kind::Emitter,
            3 => entry.kind == projectile::Kind::Entity,
            4 => entry.kind == projectile::Kind::Pickup || entry.pickup_role().is_some(),
            5 => entry.kind == projectile::Kind::Object,
            _ => true,
        }
    }
}

/// The assets one set of controls selects, in the order they are listed.
///
/// Pure, so what the browser finds is tested without drawing a frame. The query is matched
/// against text the index has already lowercased, so it is lowercased here rather than left to
/// each caller to remember. An asset the packages never name is listed only when `show_all` is
/// on or the query names its tag exactly, which is how a tag copied from elsewhere still
/// reaches its asset.
fn matching_assets(
    data: &discovery::Data,
    index: &SearchIndex,
    selection: Selection,
    query: &str,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let exact_graph = exact_asset_tag(&query);
    let words = query
        .split_whitespace()
        .map(|word| word.strip_prefix("0x").unwrap_or(word))
        .collect::<Vec<_>>();
    let mut indices = data
        .asset_choices
        .iter()
        .zip(&index.assets)
        .enumerate()
        .filter(|(_, (row, search))| {
            let entry = &data.effects.entries[row.index];
            selection.allows(entry)
                && (selection.show_all || exact_graph == Some(entry.graph) || search.identified)
                && words.iter().all(|word| index.matches(search, word))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    indices.sort_by_cached_key(|&row| {
        let entry = &data.effects.entries[data.asset_choices[row].index];
        let asset = &index.assets[row];
        // A result whose own name answers the query leads the rest, so the asset someone typed
        // the name of is not sorted behind everything that merely mentions it. The game's name
        // for it is its own name too.
        let named = |name: &str| words.iter().all(|word| name.contains(word));
        let direct_match = !query.is_empty()
            && (exact_graph == Some(entry.graph)
                || named(&asset.name)
                || asset
                    .game
                    .as_ref()
                    .is_some_and(|game| named(&game.to_lowercase())));
        asset_sort_key(selection.order, entry, asset, direct_match)
    });
    indices
}

impl Browser<'_> {
    /// `current` is the asset the action uses, which the picker selects and scrolls to when it
    /// opens, with its family of variants open.
    pub fn draw(
        &self,
        ui: &mut egui::Ui,
        scope: AssetScope,
        query: &mut String,
        reset: bool,
        use_label: Option<&str>,
        current: Option<u32>,
    ) -> Option<Asset> {
        if let Some(error) = &self.discovery.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(data) = &self.discovery.data else {
            if self.discovery.busy() {
                ui.spinner();
                ui.label("Reading game assets…");
                if let Some((current, total)) = self.discovery.progress {
                    ui.small(format!("{current} of {total} resources"));
                }
            } else {
                ui.label("No game content.");
            }
            return None;
        };
        let filter_id = ui.make_persistent_id("asset-kind");
        let mut filter = ui.data(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
        let order_id = ui.make_persistent_id("asset-order");
        let mut order =
            Order::from_index(ui.data(|state| state.get_temp::<u8>(order_id).unwrap_or_default()));
        let before = (filter, order);
        let mut visibility = (false, false);
        let mut search_changed = reset;
        // The search box takes a line of its own at the picker's width. The type, sort and
        // visibility controls follow on a line that wraps in a narrow window.
        ui.horizontal(|ui| {
            let width = ui.available_width() - pickers::CLEAR_WIDTH;
            let hint = match scope {
                AssetScope::Projectiles => "Search Projectiles or Weapons",
                AssetScope::Spawnable => "Search Objects, Effects, or Perks",
                AssetScope::Any | AssetScope::DropEffect if use_label.is_none() => {
                    "Search Objects, Effects, Perks, or Tags"
                }
                AssetScope::Any | AssetScope::DropEffect => "Search Effects or Perks",
            };
            search_changed |= sundial::ui::catalog::search(ui, query, reset, width, hint);
        });
        let count_rect = ui
            .horizontal_wrapped(|ui| {
                // A combo takes the width of its selected text unless something bounds it,
                // so each one below is drawn inside an allocation of its own width and
                // truncates to that, with the full reading on hover.
                const TYPE_WIDTH: f32 = 150.0;
                const ORDER_WIDTH: f32 = 150.0;
                const COUNT_WIDTH: f32 = 86.0;
                if scope != AssetScope::Projectiles {
                    let types = [
                        (0, "All Types"),
                        (1, "Projectiles"),
                        (2, "Emitters"),
                        (3, "Other Entities"),
                        (4, "Pickups"),
                        (5, "World Objects"),
                    ];
                    let chosen = types
                        .iter()
                        .find(|(value, _)| *value == filter)
                        .map_or("All Types", |(_, label)| *label);
                    controls::sized(ui, TYPE_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("asset-type-filter")
                            .width(TYPE_WIDTH)
                            .truncate()
                            .selected_text(chosen)
                            .show_ui(ui, |ui| {
                                for (value, name) in types {
                                    if value != 3
                                        || matches!(scope, AssetScope::Any | AssetScope::DropEffect)
                                    {
                                        ui.selectable_value(&mut filter, value, name);
                                    }
                                }
                            })
                            .response
                            .on_hover_text(format!("Asset Type: {chosen}"));
                        pickers::name_combo(ui, "asset-type-filter", "Asset Type");
                    });
                }
                let sort = format!("Sort: {}", order.label());
                controls::sized(ui, ORDER_WIDTH, |ui| {
                    egui::ComboBox::from_id_salt("asset-order")
                        .width(ORDER_WIDTH)
                        .truncate()
                        .selected_text(sort.clone())
                        .show_ui(ui, |ui| {
                            for choice in Order::ALL {
                                ui.selectable_value(&mut order, choice, choice.label())
                                    .on_hover_text(choice.hint());
                            }
                        })
                        .response
                        .on_hover_text(sort.clone());
                    pickers::name_combo(ui, "asset-order", "Sort Order");
                });
                visibility = pickers::show_all(ui, ("asset", scope as u8));
                ui.allocate_exact_size(
                    egui::vec2(COUNT_WIDTH, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                )
                .0
            })
            .inner;
        let order_index = Order::ALL
            .iter()
            .position(|choice| *choice == order)
            .unwrap_or(0) as u8;
        ui.data_mut(|state| {
            state.insert_temp(filter_id, filter);
            state.insert_temp(order_id, order_index);
        });
        let normalized_query = query.trim().to_lowercase();
        // The rows below explain why an asset matched, which needs the same words the search
        // used. Selecting the results needs nothing else from here.
        let words = normalized_query
            .split_whitespace()
            .map(|word| word.strip_prefix("0x").unwrap_or(word))
            .collect::<Vec<_>>();
        // Filtering and ordering walk every asset, so the result is kept until the query,
        // filter, order, visibility or catalog changes, and the walk itself reads one
        // prepared lowercase haystack per asset rather than lowercasing labels, hints, perk
        // names and behavior text on every keystroke. Frames in between only draw the rows
        // in view.
        let index = self.search_index(ui, data);
        let cache_id = ui.make_persistent_id("asset-results");
        // The picker's id follows its position, so one slot can serve pickers of any scope.
        let key = (
            normalized_query.clone(),
            scope as u8,
            filter,
            order_index,
            visibility.0,
            std::sync::Arc::as_ptr(&index) as usize,
        );
        let results = ui
            .data(|state| state.get_temp::<ResultCache>(cache_id))
            .filter(|cache| cache.0 == key)
            .unwrap_or_else(|| {
                let selection = Selection {
                    scope,
                    filter,
                    order,
                    show_all: visibility.0,
                };
                let indices = matching_assets(data, &index, selection, &normalized_query);
                // An empty list is usually the search, but it can also be the default listing
                // hiding what the packages never name. Counting the difference costs one more
                // walk, and only when there is nothing to show anyway.
                let hidden = if indices.is_empty() && !visibility.0 {
                    let selection = Selection {
                        show_all: true,
                        ..selection
                    };
                    matching_assets(data, &index, selection, &normalized_query).len()
                } else {
                    0
                };
                let labels = indices
                    .iter()
                    .map(|&row| self.label(&data.effects.entries[data.asset_choices[row].index]))
                    .collect::<Vec<_>>();
                let families = variants::group(labels.iter().map(String::as_str));
                let cache: ResultCache = std::sync::Arc::new((key, indices, hidden, families));
                ui.data_mut(|state| state.insert_temp(cache_id, cache.clone()));
                cache
            });
        let choices = results
            .1
            .iter()
            .map(|&index| &data.asset_choices[index])
            .collect::<Vec<_>>();
        let search = &index.assets;
        sundial::ui::catalog::toolbar_status(
            ui,
            count_rect,
            if choices.len() == 1 {
                "1 Result".to_owned()
            } else {
                format!("{} Results", choices.len())
            },
        );
        let unidentified = index.unidentified;
        if matches!(scope, AssetScope::Any | AssetScope::DropEffect)
            && use_label.is_none()
            && normalized_query.is_empty()
            && filter == 0
            && unidentified > 0
        {
            ui.weak(if visibility.0 {
                format!("{unidentified} Unnamed Assets Included")
            } else {
                format!("{unidentified} Unnamed Assets Hidden")
            });
        }
        // The names people try first are abilities and weapons, which are not objects here.
        if choices.is_empty() && !normalized_query.is_empty() && scope != AssetScope::Projectiles {
            ui.weak("Abilities are on the ability triggers and actions. Weapons are on Unique Weapon Behavior.");
        }
        ui.separator();
        // Assets that differ only by their variant number share one row, which opens to them.
        let open_id = ui.make_persistent_id("asset-families-open");
        let entry_at = |position: usize| &data.effects.entries[choices[position].index];
        // Opening the picker shows the asset in use, with its family open to it.
        let reveal = current.filter(|_| reset).and_then(|graph| {
            (0..choices.len()).find(|&position| entry_at(position).graph == graph)
        });
        if let Some(group) = reveal.and_then(|position| variants::family_of(&results.3, position)) {
            variants::open(ui.ctx(), open_id, group.key);
        }
        let open = variants::opened(ui.ctx(), open_id);
        let shown = variants::shown(&results.3, &open);
        let keys = shown
            .iter()
            .map(|row| row.key(|position| entry_at(position).graph))
            .collect::<Vec<_>>();
        let hidden = results.2;
        let mut toggled = None;
        let picked = pickers::BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(110.0),
            reset: search_changed || visibility.1 || (filter, order) != before,
            row_height: sundial::investment::authoring_choice_row_height(ui),
            select: reveal.map(|position| u64::from(entry_at(position).graph)),
        }
        .draw_body_activating(
            ui,
            |ui, index, selected| {
                let row = shown[index];
                let position = row.position();
                let entry = entry_at(position);
                let name = self.label(entry);
                // A label that uses a native name, such as Thermal Hammer, says what the game
                // calls the asset, which is also why Suggested lists it first.
                let game = search[results.1[position]].game.as_deref();
                // The row keeps the asset's kind and source. Its tag stays in the detail
                // pane, where Technical Details already show it, and in the search.
                let mut detail = without_tag(&technical_name(entry)).to_owned();
                let title = match row {
                    variants::Shown::Family(group, _) => {
                        let (title, source) = variants::family_text(group, &detail);
                        detail = source;
                        title
                    }
                    variants::Shown::Variant(_) => variants::variant_title(&name),
                    variants::Shown::Asset(_) => name.clone(),
                };
                if let Some(game) = game {
                    detail = format!("{detail} · Part of {game}");
                }
                if !matches!(row, variants::Shown::Family(..))
                    && let Some(reason) = self.match_reason(entry, &name, game, &words)
                {
                    detail = format!("{detail} · {reason}");
                }
                let draw = |ui: &mut egui::Ui| {
                    if let Some(catalog) = self.catalog {
                        catalog.draw_authoring_choice_row(ui, None, &title, Some(&detail), selected)
                    } else {
                        sundial::investment::draw_asset_choice_row(ui, &title, &detail, selected)
                    }
                };
                let response = match row {
                    variants::Shown::Variant(_) => variants::indented(ui, draw),
                    variants::Shown::Family(..) => variants::with_caret_room(ui, draw),
                    variants::Shown::Asset(_) => draw(ui),
                };
                if let variants::Shown::Family(group, opened) = row {
                    variants::paint_caret(ui, &response, opened, selected);
                    // A double-click opens a family, so its second click, which lands on the
                    // family the first click selected, leaves it open.
                    if (response.clicked() && !(response.double_clicked() && selected))
                        || variants::keyboard_toggle(ui, selected, opened)
                    {
                        toggled = Some(group.key);
                    }
                }
                response
            },
            |ui, index, activated| {
                let row = shown[index];
                // A double-click uses an asset, as its Use button does. On a family it only
                // opens the family.
                let activated = activated && !matches!(row, variants::Shown::Family(..));
                let position = row.position();
                let entry = entry_at(position);
                let name = self.label(entry);
                let game = search[results.1[position]].game.as_deref();
                if let Some(reason) = self.match_reason(entry, &name, game, &words) {
                    ui.small(format!("Matched through its source. {reason}."));
                }
                ui.heading(&name);
                // A family shows its first variant, which its Use button takes.
                if let variants::Shown::Family(group, _) = row {
                    ui.weak(format!("1 of {} Variants", group.members.len()));
                }
                if let Some(game) = game {
                    ui.label(format!("Part of {game}"));
                }
                ui.weak(format!(
                    "Name Source: {}",
                    entry.discovery_name_source_with(
                        |index| self.perk_names.get(&index).cloned(),
                        |item| self.item_names.get(&item).cloned(),
                    )
                ));
                sundial::ui::model_preview::selection(
                    ui,
                    self.discovery.packages(),
                    entry.graph,
                    &name,
                );
                ui.label(
                    entry
                        .pickup_role()
                        .map_or_else(|| entry.kind.label().to_owned(), str::to_owned),
                );
                if use_label.is_some() {
                    ui.label(scope.placement_hint(entry.kind));
                }
                if let Some(use_label) = use_label
                    && (ui.add(crate::app::style::primary(ui, use_label)).clicked() || activated)
                {
                    return Some(Asset {
                        graph: entry.graph,
                        path: entry.native_paths.first().cloned().unwrap_or_default(),
                        values: Vec::new(),
                    });
                }
                asset_details(
                    ui,
                    entry,
                    &data.effects,
                    &asset_usage(entry, self.perk_names, self.discovery),
                );
                draw_object_markers(ui, self.markers, entry.graph);
                None
            },
            hidden,
        );
        if let Some(key) = toggled {
            variants::toggle(ui.ctx(), open_id, key);
        }
        picked
    }

    /// An asset's name as every picker gives it.
    fn label(&self, entry: &projectile::catalog::Entry) -> String {
        self.asset_labels
            .get(&entry.graph)
            .cloned()
            .unwrap_or_else(|| entry.discovery_label_with(|_| None, |_| None))
    }
}

/// The attachment points on one object, where the game fires, holds and emits from it. Shown
/// on the object's own page so the question does not have to be asked from the other end.
fn draw_object_markers(
    ui: &mut egui::Ui,
    index: Option<&sundial::package_authoring::gear_markers::MarkerIndex>,
    entity: u32,
) {
    let Some(object) = index.and_then(|index| index.object(entity)) else {
        return;
    };
    ui.separator();
    egui::CollapsingHeader::new(format!("Markers ({})", object.markers.len()))
        .id_salt(("object-markers", entity))
        .show(ui, |ui| {
            egui::Grid::new(("object-marker-rows", entity))
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for marker in &object.markers {
                        ui.label(marker.label());
                        let [x, y, z] = marker.position;
                        ui.monospace(format!("{x:>8.4} {y:>8.4} {z:>8.4}"));
                        ui.end_row();
                    }
                });
        });
}

impl Browser<'_> {
    /// Why an entry answers a search when its own name does not: the perk, weapon, source
    /// context or behavior the words matched instead. Nothing when the name itself holds
    /// every word, so an ordinary match stays unadorned.
    fn match_reason(
        &self,
        entry: &projectile::catalog::Entry,
        name: &str,
        game: Option<&str>,
        words: &[&str],
    ) -> Option<String> {
        // The game's name for the asset is its own name too, and the row already shows it.
        let name = game
            .map_or_else(|| name.to_owned(), |game| format!("{name} {game}"))
            .to_lowercase();
        let missing = words
            .iter()
            .copied()
            .filter(|word| !name.contains(word))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return None;
        }
        let holds = |text: &str| {
            let text = text.to_lowercase();
            missing.iter().all(|word| text.contains(word))
        };
        let perks = entry
            .perk_indices
            .iter()
            .copied()
            .chain(entry.contexts.iter().filter_map(|context| context.perk))
            .collect::<BTreeSet<_>>();
        if let Some(perk) = perks
            .iter()
            .filter_map(|index| self.perk_names.get(index))
            .find(|perk| holds(perk))
        {
            return Some(format!("Referenced by {perk}"));
        }
        if let Some(item) = entry
            .contexts
            .iter()
            .filter_map(|context| context.item)
            .filter_map(|item| self.item_names.get(&item))
            .find(|item| holds(&item.name))
        {
            return Some(format!("Fired by {}", item.name));
        }
        if let Some(hint) = entry.source_hint.as_deref().filter(|hint| holds(hint)) {
            return Some(format!("Source context: {hint}"));
        }
        if let Some(perk) = perks
            .iter()
            .filter(|index| {
                self.discovery
                    .behavior(**index)
                    .is_some_and(|behavior| holds(&guidance::behavior_search(behavior)))
            })
            .filter_map(|index| self.perk_names.get(index))
            .next()
        {
            return Some(format!(
                "The behavior of {perk} mentions {}",
                missing.join(" ")
            ));
        }
        Some(format!(
            "Its native path or type mentions {}",
            missing.join(" ")
        ))
    }
}

/// A complete native tag is an explicit lookup, including entries hidden by Show All.
fn exact_asset_tag(query: &str) -> Option<u32> {
    let digits = query.trim().strip_prefix("0x").unwrap_or(query.trim());
    (digits.len() == 8)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

pub(in crate::app::custom_perks) use sundial::investment::discovery::technical_name;
/// Shared detail panel for authored actions and stock projectile replacements.
pub(in crate::app::custom_perks) use sundial::ui::catalog::assets::asset_details;

/// One row a picker shows in its default order, with every family closed.
#[cfg(test)]
pub(in crate::app::custom_perks::workbench) struct Listed {
    /// The asset's name, or a family's name without a variant number.
    pub label: String,
    /// The game's name for an asset its label names by a native name.
    pub game: Option<String>,
    /// Whether the asset is one Suggested leads with.
    pub well_known: bool,
    /// How many variants the row holds, one for an asset alone.
    pub variants: usize,
}

#[cfg(test)]
impl Browser<'_> {
    /// What a picker lists for a query in its default order, with the default visibility and
    /// every family closed.
    pub(in crate::app::custom_perks::workbench) fn listing(
        &self,
        scope: AssetScope,
        query: &str,
    ) -> Vec<Listed> {
        let data = self.discovery.data.as_ref().expect("discovered assets");
        let index = build_search_index(
            data,
            (0, 0, 0, 0, 0, 0),
            self.asset_labels,
            self.perk_names,
            self.item_names,
            self.carried,
        );
        let selection = Selection {
            scope,
            filter: 0,
            order: Order::default(),
            show_all: false,
        };
        let rows = matching_assets(data, &index, selection, query);
        let labels = rows
            .iter()
            .map(|&row| self.label(&data.effects.entries[data.asset_choices[row].index]))
            .collect::<Vec<_>>();
        variants::group(labels.iter().map(String::as_str))
            .into_iter()
            .map(|group| {
                let first = group.members[0];
                let asset = &index.assets[rows[first]];
                Listed {
                    label: if group.members.len() == 1 {
                        labels[first].clone()
                    } else {
                        group.name.clone()
                    },
                    game: asset.game.clone(),
                    well_known: asset.lead != usize::MAX,
                    variants: group.members.len(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssetScope, AssetSearch, Order, Selection, asset_sort_key, build_search_index, discovery,
        exact_asset_tag, matching_assets, suggested,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use sundial::investment::discovery::AssetChoice;
    use sundial::package_authoring::sandbox_perk::dependencies;
    use sundial::package_authoring::sandbox_perk::projectile::{self, catalog::Entry};

    const BOLT: u32 = 0x80B2_0001;
    const TRAIL: u32 = 0x80B2_0002;
    const ARC: u32 = 0x80B2_0003;
    const UNNAMED: u32 = 0x80B2_0004;
    /// The perk that names [`ARC`], which the packages leave unnamed.
    const CHAIN_PERK: u16 = 7;

    /// Four assets covering what the browser has to tell apart: two the packages name, one
    /// named only by the perk that uses it, and one they never name at all.
    fn catalog() -> (
        discovery::Data,
        BTreeMap<u32, String>,
        BTreeMap<u16, String>,
    ) {
        let named = |graph, kind, name: &str| Entry {
            native_name: Some(name.to_owned()),
            ..entry(graph, kind, "sandbox", None)
        };
        let mut used_by_a_perk = entry(ARC, projectile::Kind::Projectile, "sandbox", None);
        used_by_a_perk.perk_indices = vec![CHAIN_PERK];
        let entries = vec![
            named(BOLT, projectile::Kind::Projectile, "solar_bolt"),
            named(TRAIL, projectile::Kind::Emitter, "solar_trail"),
            used_by_a_perk,
            entry(UNNAMED, projectile::Kind::Object, "activities", None),
        ];
        let asset_choices = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| AssetChoice {
                index,
                name: entry.label(),
                // What the catalog prepares for each row, kept distinct so a match through it
                // is told apart from a match through the label or a perk.
                search: format!("row{index} tag{:08x}", entry.graph),
            })
            .collect();
        let perks = (0..=usize::from(CHAIN_PERK))
            .map(|index| dependencies::Perk {
                index,
                hash: 0x8080_0000 + index as u32,
                runtime_key: 0,
                action: None,
                graphs: Vec::new(),
                error: None,
                behavior: (index == usize::from(CHAIN_PERK)).then(|| dependencies::Behavior {
                    headline: "Kills release a lightning arc".to_owned(),
                    support: sundial::package_authoring::sandbox_perk::nodes::Support::Authorable,
                    editable: true,
                    program: None,
                    condition_kinds: Vec::new(),
                    effect_kinds: Vec::new(),
                    details: Vec::new(),
                    notes: Vec::new(),
                }),
            })
            .collect();
        let data = discovery::Data {
            names: Arc::new(Default::default()),
            effects: Arc::new(projectile::catalog::Catalog {
                entries,
                ..Default::default()
            }),
            asset_choices,
            perks: Arc::new(dependencies::Index {
                patterns: Vec::new(),
                caster: None,
                perks,
            }),
            perk_assets: Vec::new(),
            pattern_items: Default::default(),
            abilities: Vec::new(),
            perk_search: Default::default(),
            scripts: Arc::new(Vec::new()),
        };
        let labels = BTreeMap::from([
            (BOLT, "Solar Bolt".to_owned()),
            (TRAIL, "Solar Trail".to_owned()),
        ]);
        let perk_names = BTreeMap::from([(CHAIN_PERK, "Chain Reaction".to_owned())]);
        (data, labels, perk_names)
    }

    /// The assets one search selects, by tag, in a stable order for comparison.
    fn found(query: &str, filter: u8, show_all: bool) -> Vec<u32> {
        found_in(AssetScope::Any, query, filter, show_all)
    }

    fn found_in(scope: AssetScope, query: &str, filter: u8, show_all: bool) -> Vec<u32> {
        let (data, labels, perk_names) = catalog();
        let index = build_search_index(
            &data,
            (0, 0, 0, 0, 0, 0),
            &labels,
            &perk_names,
            &BTreeMap::new(),
            None,
        );
        let selection = Selection {
            scope,
            filter,
            order: Order::Name,
            show_all,
        };
        let mut graphs = matching_assets(&data, &index, selection, query)
            .into_iter()
            .map(|row| data.effects.entries[data.asset_choices[row].index].graph)
            .collect::<Vec<_>>();
        graphs.sort_unstable();
        graphs
    }

    /// The default listing is evidence, not everything the packages contain: an asset nothing
    /// names is noise until the reader asks for it.
    #[test]
    fn the_listing_hides_assets_nothing_names_until_show_all() {
        assert_eq!(found("", 0, false), [BOLT, TRAIL, ARC]);
        assert_eq!(found("", 0, true), [BOLT, TRAIL, ARC, UNNAMED]);
    }

    /// A tag copied from a crash report or another tool has to reach its asset, whether or not
    /// anything names it.
    #[test]
    fn an_exact_tag_reaches_an_asset_the_listing_hides() {
        assert_eq!(found("80b20004", 0, false), [UNNAMED]);
        assert_eq!(found("0x80b20004", 0, false), [UNNAMED]);
        assert_eq!(found("80B20004", 0, false), [UNNAMED]);
    }

    /// The three places a search has to reach: the catalog's own text, the label the workbench
    /// gives an asset, and the perk that uses it.
    #[test]
    fn a_search_reaches_the_catalog_text_the_label_and_the_perk() {
        assert_eq!(found("row1", 0, false), [TRAIL], "the catalog's own text");
        assert_eq!(found("solar bolt", 0, false), [BOLT], "the workbench label");
        assert_eq!(found("chain", 0, false), [ARC], "the perk's name");
        assert_eq!(found("lightning", 0, false), [ARC], "the perk's behavior");
    }

    /// Every word has to be found, or a two word search is no narrower than its first word.
    #[test]
    fn every_word_of_a_search_has_to_match() {
        assert_eq!(found("solar", 0, false), [BOLT, TRAIL]);
        assert_eq!(found("solar trail", 0, false), [TRAIL]);
        assert!(found("solar lightning", 0, false).is_empty());
        assert!(found("nothing here", 0, false).is_empty());
    }

    /// Sorting never hides a result, but the type filter is meant to.
    #[test]
    fn the_type_filter_selects_one_kind_and_the_projectile_scope_ignores_it() {
        assert_eq!(found("", 1, true), [BOLT, ARC], "projectiles");
        assert_eq!(found("", 2, true), [TRAIL], "emitters");
        assert_eq!(found("", 5, true), [UNNAMED], "world objects");
        // The projectile picker is already limited to one type, so its combo is not drawn and
        // a filter left over from the catalog must not narrow it further.
        assert_eq!(
            found_in(AssetScope::Projectiles, "", 2, true),
            [BOLT, ARC],
            "a stale emitter filter cannot empty the projectile picker"
        );
    }

    /// Everything a search can match is lowercased once, so a query never has to be.
    #[test]
    fn a_search_is_insensitive_to_case() {
        assert_eq!(found("SOLAR Bolt", 0, false), [BOLT]);
        assert_eq!(found("Chain", 0, false), [ARC]);
    }

    fn entry(graph: u32, kind: projectile::Kind, package: &str, path: Option<&str>) -> Entry {
        Entry {
            graph,
            kind,
            object_type: 18,
            owners: Vec::new(),
            package: package.into(),
            native_name: None,
            native_paths: path.map(|path| vec![path.to_owned()]).unwrap_or_default(),
            contexts: Vec::new(),
            perk_indices: Vec::new(),
            source_hint: None,
        }
    }

    #[test]
    fn every_asset_order_is_total_and_keeps_the_same_result_set() {
        let named = entry(
            0x80B1_0001,
            projectile::Kind::Emitter,
            "sandbox",
            Some("content/sandbox/effects/zebra.pattern.tft"),
        );
        let unnamed = entry(
            0x80B1_0002,
            projectile::Kind::Projectile,
            "activities",
            None,
        );
        let mut rows = [(&named, "zebra emitter"), (&unnamed, "alpha projectile")];
        fn order_by<'a>(order: Order, rows: &mut [(&Entry, &'a str)]) -> Vec<&'a str> {
            rows.sort_by_cached_key(|(entry, name)| {
                asset_sort_key(order, entry, &searched(name), false)
            });
            rows.iter().map(|(_, name)| *name).collect()
        }
        // Suggested ranks an asset the packages name above one they do not.
        assert_eq!(
            order_by(Order::Suggested, &mut rows),
            ["zebra emitter", "alpha projectile"]
        );
        assert_eq!(
            order_by(Order::Name, &mut rows),
            ["alpha projectile", "zebra emitter"]
        );
        // Emitter sorts before Projectile, and activities before sandbox.
        assert_eq!(
            order_by(Order::Kind, &mut rows),
            ["zebra emitter", "alpha projectile"]
        );
        assert_eq!(
            order_by(Order::Source, &mut rows),
            ["alpha projectile", "zebra emitter"]
        );
    }

    /// The search text of an asset with only a label.
    fn searched(name: &str) -> AssetSearch {
        AssetSearch {
            name: name.into(),
            own: String::new(),
            perks: Vec::new(),
            identified: true,
            game: suggested::game_name(name),
            lead: suggested::rank(name),
            uses: 0,
        }
    }

    #[test]
    fn a_query_match_outranks_a_named_asset_only_under_suggested() {
        let named = entry(
            0x80B1_0001,
            projectile::Kind::Projectile,
            "sandbox",
            Some("content/sandbox/effects/named.pattern.tft"),
        );
        let matched = entry(0x80B1_0002, projectile::Kind::Projectile, "sandbox", None);
        assert!(
            asset_sort_key(Order::Suggested, &matched, &searched("hit"), true)
                < asset_sort_key(Order::Suggested, &named, &searched("aaa"), false)
        );
        assert!(
            asset_sort_key(Order::Name, &matched, &searched("hit"), true)
                > asset_sort_key(Order::Name, &named, &searched("aaa"), false)
        );
    }

    /// A Discord report: Firefly's action list said "Firefly Attachment 2" and the picker
    /// could not be searched for it. The entity is attached by Firefly and the Ace of
    /// Spades Catalyst, so both names now say so, and the picker's search finds it.
    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn a_stock_attachment_is_searchable_by_the_name_its_perk_gives_it() {
        use std::path::PathBuf;
        let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
        let catalog =
            sundial::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
                .unwrap();
        let names = catalog
            .weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition)
            .into_iter()
            .map(|choice| (choice.perk_index, choice.representative_name))
            .collect::<std::collections::BTreeMap<_, _>>();
        let data = sundial::investment::discovery::discover(&packages, |_| {}).unwrap();
        let labels = data
            .effects
            .discovery_labels_with(|index| names.get(&index).cloned(), |_| None);
        let firefly = data
            .effects
            .entries
            .iter()
            .find(|entry| entry.graph == 0x80C1_9A55)
            .expect("Firefly's explosion attachment");
        assert!(firefly.has_discovery_identity_with(|index| names.get(&index).cloned(), |_| None));
        let label = &labels[&firefly.graph];
        assert_eq!(
            label,
            "Attachment Shared by Ace of Spades Catalyst, Firefly"
        );
        assert!(crate::app::pickers::matches("firefly attachment", label));
        // Every attachment a named weapon perk carries reads as one.
        let attachments = data
            .effects
            .entries
            .iter()
            .filter(|entry| {
                entry.kind == projectile::Kind::Entity
                    && entry.direct_name().is_none()
                    && entry
                        .source_hint
                        .as_deref()
                        .is_some_and(|role| role.starts_with("Attached Entity"))
                    && entry
                        .perk_indices
                        .iter()
                        .any(|index| names.contains_key(index))
            })
            .collect::<Vec<_>>();
        assert!(!attachments.is_empty());
        for entry in &attachments {
            let label = &labels[&entry.graph];
            assert!(
                label.contains("Attachment") && !label.starts_with("Unidentified"),
                "0x{:08X}: {label}",
                entry.graph
            );
        }
        println!(
            "{} attachments carried by named weapon perks are named after them",
            attachments.len()
        );
    }

    #[test]
    fn only_complete_asset_tags_bypass_discovery_visibility() {
        assert_eq!(exact_asset_tag("0x80c1d182"), Some(0x80C1D182));
        assert_eq!(exact_asset_tag("80c1d182"), Some(0x80C1D182));
        for query in ["frog", "80c1", "80c1d182 frog", "123456789", "zzzzzzzz"] {
            assert_eq!(exact_asset_tag(query), None);
        }
    }
}
