//! The perks saved recipes carry, each variant once with the perk it resolves to, and what
//! Custom Perks gains from them. Reading every saved recipe takes seconds in a large library, so
//! each variant is resolved once here and windows filter, select and show what this holds.
use super::{Entry, Library, PerkRecipe, Scan, embedded::Incoming};
use crate::{WeaponRecipe, WeaponSocketPlugVariantRecipe, recipe_library::RecipeLibrary};
use std::collections::{BTreeMap, BTreeSet};
use sundial::investment::InvestmentCatalog;

/// How many saved recipes are held at once while their perks are collected.
const RECIPES_AT_ONCE: usize = 64;

/// One perk a saved recipe, or the open one, carries.
pub(crate) struct SavedPerk {
    /// The name of the recipe it comes from.
    pub(crate) weapon: String,
    pub(crate) kind: crate::ItemKind,
    /// The variant as the recipe holds it, at socket and choice zero.
    pub(crate) variant: WeaponSocketPlugVariantRecipe,
    /// The perk the variant resolves to (`PerkRecipe::from_variant`).
    pub(crate) perk: PerkRecipe,
    /// Whether it comes from the open recipe rather than a saved one.
    pub(crate) draft: bool,
}

/// The perks of every saved recipe, how many imported perks Custom Perks gained, the imports it no
/// longer needs, which the window removes, the library as the import left it, and what could not
/// be read.
pub(crate) struct Loaded {
    pub(crate) perks: SavedPerks,
    pub(crate) added: usize,
    pub(crate) redundant: Vec<Entry>,
    /// The library read after the import, with its import history. It stands for the library
    /// while no perk file changed since (`Library::unchanged_since`), so the window need not read
    /// every file again.
    pub(crate) library: Option<(Scan, Result<BTreeSet<String>, String>)>,
    pub(crate) warnings: Vec<String>,
}

/// Reads every saved recipe's perks, adds the ones the Custom Perks library `perks` does not hold
/// yet and finds the imports those perks replaced.
pub(crate) fn load(
    perks: Option<&Library>,
    library: Option<&RecipeLibrary>,
    catalog: &InvestmentCatalog,
) -> Loaded {
    let (saved, mut warnings) = SavedPerks::read(library, catalog);
    let mut added = 0;
    let mut redundant = Vec::new();
    let mut read = None;
    if let Some(perks) = perks {
        let incoming = Incoming::new(saved.list.iter().filter(|entry| {
            let hash = entry.variant.source_plug_hash.parse_u32().unwrap_or_default();
            let unavailable = !entry.variant.replace_effects && catalog.item_definition_tag(hash).is_none();
            if unavailable {
                warnings.push(format!("Could not import a custom perk from {} because its stock template {hash:08X} is unavailable. The recipe is unchanged.", entry.weapon));
            }
            !unavailable
        }).map(|entry| &entry.perk));
        // A recipe or perk that could not be read may still carry an import, so none is removed.
        let complete = warnings.is_empty();
        match perks.import_incoming(&incoming) {
            Ok(report) => {
                warnings.extend(report.errors);
                added = report.added;
                if complete {
                    match perks.redundant_imports(&incoming) {
                        Ok(found) => redundant = found,
                        Err(error) => warnings.push(error),
                    }
                }
            }
            Err(error) => warnings.push(error),
        }
        // A scan that fails leaves the window to read the library itself, which reports why.
        read = perks.scan().ok().map(|scan| (scan, perks.imported_ids()));
    }
    Loaded {
        perks: saved,
        added,
        redundant,
        library: read,
        warnings,
    }
}

/// The saved perks, each variant once, in the order they were first found.
#[derive(Default)]
pub(crate) struct SavedPerks {
    pub(crate) list: Vec<SavedPerk>,
    /// Each perk's index by its stock plug and name. A library of imported weapons holds tens of
    /// thousands of variants, too many to compare each with every one before it.
    by_plug: BTreeMap<(crate::recipe::HexHash, Option<String>), Vec<usize>>,
}

impl SavedPerks {
    /// The perks of every recipe `library` saved, with what could not be read.
    pub(crate) fn read(
        library: Option<&RecipeLibrary>,
        catalog: &InvestmentCatalog,
    ) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut saved = Self::default();
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
                            saved.append(recipe, &linked, catalog, false);
                        }
                    }
                }
                Err(error) => warnings.push(error),
            }
        }
        (saved, warnings)
    }

    /// The perks of the open recipe `draft`, as a saved recipe's read.
    pub(crate) fn of_draft(draft: &WeaponRecipe, catalog: &InvestmentCatalog) -> Vec<SavedPerk> {
        let mut saved = Self::default();
        saved.append_draft(draft, catalog);
        saved.list
    }

    pub(crate) fn append_draft(&mut self, draft: &WeaponRecipe, catalog: &InvestmentCatalog) {
        let linked = LinkedIcons::read(std::slice::from_ref(draft));
        self.append(draft.clone(), &linked, catalog, true);
    }

    fn append(
        &mut self,
        recipe: WeaponRecipe,
        linked: &LinkedIcons,
        catalog: &InvestmentCatalog,
        draft: bool,
    ) {
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
            self.list.push(SavedPerk {
                weapon: recipe.name.clone(),
                kind: recipe.kind,
                perk: PerkRecipe::from_variant(&variant, catalog),
                variant,
                draft,
            });
        }
    }
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
