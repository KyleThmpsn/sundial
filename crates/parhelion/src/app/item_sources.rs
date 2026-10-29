//! Where installed items come from, read beside the catalog. Builds read stock alone, so an item
//! another Sunrise mod adds can't be a base, and a base whose own records another mod changes
//! builds from its stock records.
use super::*;
use sundial::package_authoring::mod_content::{self, Provenance, Source, is_family_id};

/// The mods the loader admits and what each adds or changes. Empty without installed mods.
#[derive(Clone, Default)]
pub(super) struct ItemSources(Option<Arc<Provenance>>);

/// Where one installed item comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ItemSource<'a> {
    Stock,
    /// A Parhelion build installed it.
    Built,
    /// Another mod's new package holds its definition.
    Added(&'a Source),
    /// A stock item whose definition or strings these mods change, in load order.
    Changed(Vec<&'a Source>),
}

impl ItemSources {
    /// What the installation's mods add and change, as the catalog's readers admit them.
    pub(super) fn read(install: &Path) -> Result<Self, String> {
        Ok(Self(
            mod_content::View::open(install)?.map(|view| Arc::new(view.provenance().clone())),
        ))
    }

    pub(super) fn of(&self, catalog: &InvestmentCatalog, hash: u32) -> ItemSource<'_> {
        let Some(definition) = catalog.item_definition_tag(hash) else {
            return ItemSource::Stock;
        };
        let provenance = self.0.as_deref();
        if let Some(source) = provenance.and_then(|p| p.added_by(TagHash(definition))) {
            return if is_family_id(&source.id) {
                ItemSource::Built
            } else {
                ItemSource::Added(source)
            };
        }
        // Earlier layouts install Parhelion's records beside the stock ones.
        if !crate::package_profile::is_stock_item_definition(definition) {
            return ItemSource::Built;
        }
        let Some(provenance) = provenance else {
            return ItemSource::Stock;
        };
        let mut changed = Vec::new();
        let records = std::iter::once(definition).chain(catalog.item_string_tag(hash));
        for source in records.flat_map(|tag| provenance.changed_by(TagHash(tag))) {
            if !is_family_id(&source.id) && !changed.contains(&source) {
                changed.push(source);
            }
        }
        if changed.is_empty() {
            ItemSource::Stock
        } else {
            ItemSource::Changed(changed)
        }
    }

    /// Whether a build can resolve `hash` from the game's own tables.
    pub(super) fn is_stock(&self, catalog: &InvestmentCatalog, hash: u32) -> bool {
        matches!(
            self.of(catalog, hash),
            ItemSource::Stock | ItemSource::Changed(_)
        )
    }

    /// Whether a base picker lists `hash`. Another mod's own items are left out.
    pub(super) fn listed(&self, catalog: &InvestmentCatalog, hash: u32) -> bool {
        !matches!(self.of(catalog, hash), ItemSource::Added(_))
    }
}

impl PackageAuthoringApp {
    /// The line under a base card when another mod adds or changes the base.
    pub(super) fn draw_base_source(&self, ui: &mut egui::Ui, hash: Option<u32>) {
        let (Some(catalog), Some(hash)) = (self.catalog.as_ref(), hash) else {
            return;
        };
        let kind = self.recipe.kind;
        match self.item_sources.of(catalog, hash) {
            ItemSource::Added(source) => style::problem(
                ui,
                &added_base_problem(
                    &self.recipe.name,
                    &base_name(catalog, &self.recipe.donor),
                    &source.name,
                    kind,
                ),
            ),
            ItemSource::Changed(sources) => {
                style::warning(ui, &changed_base_warning(&sources, kind));
            }
            ItemSource::Stock | ItemSource::Built => {}
        }
    }
}

/// The base's name as the recipe saved it, else as the catalog names it.
pub(super) fn base_name(catalog: &InvestmentCatalog, donor: &WeaponDonorReference) -> String {
    donor
        .expected_name
        .clone()
        .or_else(|| {
            donor
                .item_hash
                .parse_u32()
                .ok()
                .and_then(|hash| catalog.item_display_name(hash))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| donor.item_hash.to_string())
}

/// Why a recipe can't build on `base`, which `source` adds. The build stops with the same text.
pub(super) fn added_base_problem(recipe: &str, base: &str, source: &str, kind: ItemKind) -> String {
    format!(
        "{recipe} builds on {base}, which {source} adds. Choose a stock or library {} as its base.",
        noun(kind)
    )
}

/// The card's line for a base other mods change.
pub(super) fn changed_base_warning(sources: &[&Source], kind: ItemKind) -> String {
    let verb = if sources.len() == 1 {
        "changes"
    } else {
        "change"
    };
    format!(
        "{} {verb} this {}. Builds use the stock version.",
        spoken_names(sources),
        noun(kind)
    )
}

fn spoken_names(sources: &[&Source]) -> String {
    let names = sources
        .iter()
        .map(|source| source.name.as_str())
        .collect::<Vec<_>>();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn noun(kind: ItemKind) -> &'static str {
    if kind.is_weapon() { "weapon" } else { "item" }
}
