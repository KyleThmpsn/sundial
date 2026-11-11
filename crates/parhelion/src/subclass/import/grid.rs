//! Saved source subclass candidates on the native solar Warlock layout. The private Super and
//! every source grid choice with a complete translation take their source names, text and icons.
//! Each tree takes the Super upgrade, melee upgrade and passive choices of its source row, and the
//! name of that row's attribute choice. A choice whose behavior does not translate leaves its
//! native node in place, so no node presents a source choice it does not perform.
use super::*;
use crate::icon_edit::ImportedIcon;
use crate::perk::Icon;
use crate::subclass::{
    AbilityModifier, AttunementPath, EntryIcon, ModifierEffect, SubclassPathNode,
};
use parhelion_import::d1_roi::ability::grid::Step;
use std::collections::BTreeMap;
use sundial::investment::{SubclassSummary, WeaponDamageType};
use sundial::package_authoring::sandbox_perk::program::{Action, PropertyOperation};

/// Trees from top to bottom, matching the source grid's rows from top to bottom.
const PATHS: [AttunementPath; 3] = [
    AttunementPath::Top,
    AttunementPath::Middle,
    AttunementPath::Bottom,
];

/// The source slot byte of a grant that equips a melee.
const MELEE_SLOT: u8 = 2;

/// Where a tree's nodes go. Top and bottom lead with their melee and have three passives. The
/// middle tree leads with its own Super, so its Super upgrade joins that node.
struct Tree {
    melee: u8,
    passives: Vec<u8>,
    upgrade: Option<u8>,
}

fn tree(path: AttunementPath) -> Tree {
    if path == AttunementPath::Middle {
        Tree {
            melee: 1,
            passives: vec![2, 3],
            upgrade: None,
        }
    } else {
        Tree {
            melee: 0,
            passives: vec![1, 2],
            upgrade: Some(3),
        }
    }
}

/// The donor entry's own perks and modifiers, which a replaced choice leaves out.
fn removed(donor: &SubclassSummary, entry: u8) -> (Vec<u16>, Vec<StockModifier>) {
    let perks = donor
        .entry_perks
        .get(&entry)
        .into_iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let modifiers = donor
        .entry_modifiers
        .get(&entry)
        .into_iter()
        .flatten()
        .map(|&(key, row)| StockModifier { key, row })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (perks, modifiers)
}

fn artwork(step: &Step) -> Result<Option<Icon>, String> {
    step.icon
        .as_deref()
        .map(|png| {
            Ok(Icon::Image {
                name: step.name.clone(),
                image: ImportedIcon::from_bytes(png)?,
            })
        })
        .transpose()
}

fn icon(step: &Step) -> Result<Option<EntryIcon>, String> {
    Ok(artwork(step)?.map(|artwork| EntryIcon::Artwork { artwork }))
}

/// The source HUD art of the last of `steps` naming a glyph. They apply in order, and the last
/// row the source applies decides the tile.
fn hud_artwork(steps: &[&Step]) -> Result<Option<Icon>, String> {
    steps
        .iter()
        .rev()
        .find_map(|step| Some((step, step.hud_icon.as_deref()?)))
        .map(|(step, png)| {
            Ok(Icon::Image {
                name: step.name.clone(),
                image: ImportedIcon::from_bytes(png)?,
            })
        })
        .transpose()
}

/// The square source subclass icon cut to the diamond its edge midpoints span, as Shadowkeep's
/// subclass icons are drawn. One pixel of coverage falloff smooths the cut edges.
fn diamond(png: &[u8]) -> Result<ImportedIcon, String> {
    let mut image = image::load_from_memory(png).map_err(error)?.to_rgba8();
    let (width, height) = image.dimensions();
    let (half_width, half_height) = (width as f32 / 2.0, height as f32 / 2.0);
    let scale = half_width.min(half_height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let outside = ((x as f32 + 0.5 - half_width).abs() / half_width
            + (y as f32 + 0.5 - half_height).abs() / half_height
            - 1.0)
            * scale;
        let coverage = (0.5 - outside).clamp(0.0, 1.0);
        pixel.0[3] = (f32::from(pixel.0[3]) * coverage).round() as u8;
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(error)?;
    ImportedIcon::from_bytes(&bytes.into_inner())
}

/// The step's lowered programs as one custom perk, one effect each on its own carrier, under its
/// source identity.
fn perk(steps: &[&Step], carriers: &[u16], namespace: &str) -> Result<Option<PerkRecipe>, String> {
    let programs = steps
        .iter()
        .flat_map(|step| &step.programs)
        .collect::<Vec<_>>();
    let Some(lead) = steps.last() else {
        return Ok(None);
    };
    if programs.is_empty() {
        return Ok(None);
    }
    if programs.len() > carriers.len() {
        return Err(format!(
            "{} needs {} effects, more than the {} carrier perks available",
            lead.name,
            programs.len(),
            carriers.len()
        ));
    }
    let mut perk = PerkRecipe::new();
    perk.id = digest(
        &serde_json::to_vec(&json!({"source":"d1_roi","namespace":namespace,
            "steps":steps.iter().map(|step| (step.node, step.step)).collect::<Vec<_>>()}))
        .map_err(error)?,
    );
    perk.name = lead.name.clone();
    perk.description = lead.description.clone();
    perk.icon = artwork(lead)?;
    for (program, &carrier) in programs.into_iter().zip(carriers) {
        let mut effect = PerkRecipe::effect(carrier);
        effect.program = Some(program.clone());
        perk.effects.push(effect);
    }
    Ok(Some(perk))
}

/// Steps grouped by their source group key, each group in source row order.
fn groups(grid: &[Step]) -> BTreeMap<u32, Vec<&Step>> {
    let mut groups = BTreeMap::<u32, Vec<&Step>>::new();
    for step in grid {
        groups.entry(step.group).or_default().push(step);
    }
    for steps in groups.values_mut() {
        steps.sort_by_key(|step| (step.row, step.node, step.step));
    }
    groups
}

/// Actions matched by a native equivalent the native layout already grants.
fn equivalents(step: &Step) -> bool {
    step.report["native_equivalents"]
        .as_array()
        .is_some_and(|actions| !actions.is_empty())
}

/// A source attribute choice: stat rows and nothing else. Shadowkeep has no counterpart.
fn attribute(step: &Step) -> bool {
    step.equips.is_empty()
        && step.super_keys.is_empty()
        && step.properties.is_empty()
        && step.programs.is_empty()
        && step.unsupported.is_empty()
        && !equivalents(step)
        && step.stats.as_array().is_some_and(|rows| !rows.is_empty())
}

/// Every property grant targets `row`, and the step has no other contribution kind.
fn upgrades_row(step: &Step, row: u8) -> bool {
    step.equips.is_empty()
        && step.super_keys.is_empty()
        && !step.properties.is_empty()
        && step.properties.iter().all(|&(_, target)| target == row)
}

/// Keys the step's lowered rows are gated on. Such a row applies only while its gate applies too.
fn gates(step: &Step) -> BTreeSet<u32> {
    step.report["ability_grants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|grant| grant["gate"].as_u64())
        .filter_map(|gate| u32::try_from(gate).ok())
        .collect()
}

/// Keys a lowered step's programs apply.
fn applied(steps: &[&Step]) -> BTreeSet<u32> {
    steps
        .iter()
        .flat_map(|step| &step.programs)
        .flat_map(|program| &program.actions)
        .filter_map(|action| match action {
            Action::AbilityProperty {
                key,
                option: PropertyOperation::Apply,
                ..
            } => Some(*key),
            _ => None,
        })
        .collect()
}

fn status(step: &Step, placed: &str, entries: &[u8]) -> Value {
    json!({"node":step.node,"step":step.step,"name":step.name,"placement":placed,
        "entries":entries,"hud_glyph":step.hud_glyph,"unsupported":step.unsupported,
        "stats":step.stats,"lowering":step.report})
}

/// The tree whose passive pool every placed passive and Super upgrade reuses, and its position.
/// A same-position node in another tree can equip its own movement ability even after its perks
/// are removed, so only this pool is qualified.
const PASSIVE_SOURCE: (AttunementPath, u8) = (AttunementPath::Middle, 3);

/// The native entry of the qualified passive pool.
fn passive_source_entry() -> u8 {
    PASSIVE_SOURCE.0.entries()[usize::from(PASSIVE_SOURCE.1)]
}

/// The native solar graph supplies the common controller, helpers and channel bank. Source
/// sequences replace its presentation. The public ability picker stays hidden until this
/// candidate passes package loading and complete gameplay acceptance.
pub fn prepare(request: &Request<'_>) -> Result<Prepared, String> {
    let super_entry = super_entry(request)?;
    let mut inputs = Inputs::open(request)?;
    let subclasses = inputs
        .catalog
        .subclasses(crate::package_profile::is_stock_item_definition);
    let (donor, base) = solar_donor(request, &subclasses)?;
    let translated = inputs.translate(request, donor, super_entry)?;
    let imported = inputs.publish_super(&translated.upgrades, translated.subclass.pattern)?;
    let mut recipe = subclass_recipe(request, &translated)?;
    let abilities = recipe
        .overrides
        .subclass_abilities
        .get_or_insert_with(Default::default);
    let records = place(
        request,
        (donor, base),
        &translated,
        &imported,
        super_entry,
        abilities,
    )?;
    inputs.publish(request, recipe, &translated, records)
}

/// The middle tree's own Super entry, once the request names a passive node of that tree on a
/// subclass recipe that validates.
fn super_entry(request: &Request<'_>) -> Result<u8, String> {
    let entries = AttunementPath::Middle.entries();
    if request.recipe.kind != ItemKind::Subclass
        || !matches!(
            entries.iter().position(|&entry| entry == request.entry),
            Some(2 | 3)
        )
    {
        return Err(
            "Self-revive preparation requires a passive node in the middle attunement".into(),
        );
    }
    request
        .recipe
        .to_spec()
        .map_err(error)?
        .validate()
        .map_err(error)?;
    Ok(entries[0])
}

/// The one stock solar Warlock subclass, which presents the private Super and lends every
/// replaced node, and the recipe's base subclass hash, which must be a Warlock's.
fn solar_donor<'a>(
    request: &Request<'_>,
    subclasses: &'a [SubclassSummary],
) -> Result<(&'a SubclassSummary, u32), String> {
    let donors = subclasses
        .iter()
        .filter(|candidate| {
            candidate.class_type == 2 && candidate.damage_type == Some(WeaponDamageType::Solar)
        })
        .collect::<Vec<_>>();
    let [donor] = donors.as_slice() else {
        return Err("Native solar Warlock presentation is missing or ambiguous".into());
    };
    if donor.entry_rows.contains_key(&request.entry) {
        return Err(
            "Self-revive preparation requires a native node without an equipped ability".into(),
        );
    }
    let base = request.recipe.donor.item_hash.parse_u32().map_err(error)?;
    if !subclasses
        .iter()
        .any(|candidate| candidate.hash == base && candidate.class_type == 2)
    {
        return Err("Self-revive preparation currently requires a Warlock subclass base".into());
    }
    Ok((*donor, base))
}

/// The package trees and the new output folder a preparation reads and writes, the stamp the
/// native packages had when it began, and the readers and catalog it opened.
struct Inputs {
    source_path: std::path::PathBuf,
    native_path: std::path::PathBuf,
    output: std::path::PathBuf,
    native_stamp: String,
    source: Source,
    manager: sundial::package_authoring::PackageManager,
    native: Reader,
    catalog: InvestmentCatalog,
}

/// What translating the source Super gives the grid placement.
struct Translated {
    /// The private Super with every exclusive upgrade.
    upgrades: parhelion_import::d1_roi::ability::upgrades::Prepared,
    subclass: parhelion_import::d1_roi::ability::SubclassCatalog,
    grid: Vec<Step>,
    icon: Option<Vec<u8>>,
    /// The finished sandbox perks that carry a custom perk's effects, one each.
    carriers: Vec<u16>,
    /// The native solar Super graph whose controller the private Super keeps.
    presentation: u32,
}

impl Inputs {
    /// Resolves both package trees and a new output folder before anything is written, then
    /// opens the readers and the native catalog.
    fn open(request: &Request<'_>) -> Result<Self, String> {
        let source_path = packages_path(request.source_packages).map_err(error)?;
        let native_path = request.native_packages.canonicalize().map_err(error)?;
        let output =
            outside(request.output, source_path.parent().unwrap_or(&source_path)).map_err(error)?;
        outside(&output, native_path.parent().unwrap_or(&native_path)).map_err(error)?;
        if output.exists() {
            return Err("Choose a new ability import output folder".into());
        }
        let native_stamp = package_stamp(&native_path).map_err(error)?;
        let source = Source::open(&source_path, |_, _| {}).map_err(error)?;
        let manager = open_shadowkeep_package_manager(&native_path)?;
        fs::create_dir_all(&output).map_err(error)?;
        let native =
            Reader::discovery(&native_path, &output.join("native-reads"), false).map_err(error)?;
        let catalog = InvestmentCatalog::load_with_cache_path(
            native_path
                .parent()
                .ok_or("The native packages have no installation parent")?,
            &output.join("native-catalog.json"),
            true,
            |_| {},
        )?;
        Ok(Self {
            source_path,
            native_path,
            output,
            native_stamp,
            source,
            manager,
            native,
            catalog,
        })
    }

    /// Whether both package trees still have the stamps the preparation began with.
    fn unchanged(&self) -> Result<bool, String> {
        Ok(
            package_stamp(&self.native_path).map_err(error)? == self.native_stamp
                && package_stamp(&self.source_path).map_err(error)? == self.source.report().stamp,
        )
    }

    /// The private Super, one upgrade for each tree, and the source grid it belongs to.
    fn translate(
        &mut self,
        request: &Request<'_>,
        donor: &SubclassSummary,
        super_entry: u8,
    ) -> Result<Translated, String> {
        // Top and bottom share entry 10, while the middle path owns entry 20. Both need the
        // compatible foundation. Each path selects its own exclusive source upgrade. The native
        // graph qualifies the controller envelope, not active Super or revival gameplay.
        let presentation = *donor
            .entry_entities
            .get(&super_entry)
            .ok_or("The native solar presentation has no Super graph")?;
        let upgrades = self
            .source
            .prepare_upgrades(
                &self.manager,
                &mut self.native,
                request.choice,
                presentation,
                "radiance",
            )
            .map_err(error)?;
        if upgrades.upgrades.len() != PATHS.len() {
            return Err("The source Radiance group needs one upgrade for each path".into());
        }
        let subclass = self
            .source
            .subclass_choices()
            .map_err(error)?
            .into_iter()
            .find(|subclass| {
                subclass.item == request.choice.subclass && subclass.grid == request.choice.grid
            })
            .ok_or("The source subclass changed during preparation")?;
        let folder = self.output.join("grid");
        fs::create_dir_all(&folder).map_err(error)?;
        let grid = self
            .source
            .subclass_grid(&mut self.native, &subclass, &folder, None)
            .map_err(error)?;
        let icon = self.source.subclass_icon(subclass.item).map_err(error)?;
        let carriers = self.carriers()?;
        Ok(Translated {
            upgrades,
            subclass,
            grid,
            icon,
            carriers,
            presentation,
        })
    }

    /// A node's variant selects each finished sandbox perk once, so the effects of one custom
    /// perk each take a carrier of their own. Other nodes reuse the same carriers. The default
    /// plug layout's own perk leads, as a single effect always took it.
    fn carriers(&mut self) -> Result<Vec<u16>, String> {
        let globals_tag = resolve_live_named_tag(&self.manager, "investment_globals", None)?;
        let globals = self.native.tag(globals_tag.0, None).map_err(error)?;
        let mut carriers = Vec::new();
        for index in self
            .catalog
            .item_sandbox_perk_indices(crate::perk::DEFAULT_PLUG_LAYOUT)
            .into_iter()
            .chain(crate::perk::EFFECT_CARRIERS)
        {
            if carriers.len() < crate::perk::SANDBOX_PERK_CAPACITY
                && !carriers.contains(&index)
                && load_sandbox_perk_runtime_action(&self.manager, &globals.0, usize::from(index))
                    .is_ok()
            {
                carriers.push(index);
            }
        }
        if carriers.is_empty() {
            return Err("The native private-perk template has no usable action".into());
        }
        Ok(carriers)
    }

    /// Saves the private Super where the subclass recipe names it, while both package trees are
    /// unchanged.
    fn publish_super(
        &self,
        upgrades: &parhelion_import::d1_roi::ability::upgrades::Prepared,
        identity: u32,
    ) -> Result<super::super::ImportedAbility, String> {
        let directory = self.output.join("ability");
        if !self.unchanged()? {
            return Err("The package inputs changed during ability import preparation".into());
        }
        upgrades.save(&directory).map_err(error)?;
        Ok(super::super::ImportedAbility {
            directory: directory.clone(),
            sha256: parhelion_import::d2_mot::native::attachment::fingerprint(&directory)
                .map_err(error)?,
            symbol: upgrades.root.clone(),
            // The subclass's Super pattern is its definitions row key, the identity perk
            // conditions name.
            source: Some(super::super::SourceAbility {
                source: parhelion_import::d1_roi::ABILITY_SOURCE.to_owned(),
                identity,
            }),
        })
    }

    /// Writes the prepared subclass recipe and its provenance, once it validates and both
    /// package trees are still unchanged.
    fn publish(
        &self,
        request: &Request<'_>,
        recipe: WeaponRecipe,
        translated: &Translated,
        records: Vec<Value>,
    ) -> Result<Prepared, String> {
        recipe.to_spec().map_err(error)?.validate().map_err(error)?;
        if !self.unchanged()? {
            return Err("The package inputs changed before ability candidate publication".into());
        }
        let encoded = recipe.to_json_pretty().map_err(error)?;
        let subclass = &translated.subclass;
        let provenance = json!({"status":"prepared source subclass candidate","gameplay_verified":false,
            "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
            "prepared_recipe_sha256":digest(encoded.as_bytes()),"source":self.source.report(),
            "native_stamp":self.native_stamp,"native_presentation":translated.presentation,
            "translation":translated.upgrades.evidence,
            "super_entries":[layout::SUPER,AttunementPath::Middle.entries()[0]],
            "subclass":{"item":subclass.item,"name":subclass.name,"icon":translated.icon.is_some()},
            "grid":records,
            "limits":["Source grenades, movement and melee keep their native ability graphs. Their source properties and actions are translated.",
                "Source attribute choices and stat rows have no Shadowkeep counterpart and are left out.",
                "A choice whose behavior does not translate keeps its native node. Its reasons are recorded."]});
        fs::write(
            self.output.join("provenance.json"),
            serde_json::to_vec_pretty(&provenance).map_err(error)?,
        )
        .map_err(error)?;
        fs::write(
            self.output.join("prepared-subclass.parhelion.json"),
            encoded,
        )
        .map_err(error)?;
        Ok(Prepared {
            baseline: request.recipe.clone(),
            recipe,
            provenance,
        })
    }
}

/// The request's recipe with the source subclass's name, flavor text and icon, and only its HUD
/// color kept among its authored entries. The source grid owns every entry, so a previous
/// preparation's nodes cannot linger where a choice now keeps its native node.
fn subclass_recipe(request: &Request<'_>, translated: &Translated) -> Result<WeaponRecipe, String> {
    let subclass = &translated.subclass;
    let mut recipe = request.recipe.clone();
    recipe.name = subclass.name.trim().to_owned();
    // The source subclass's description is its flavor text. Rise of Iron items have no lore tab,
    // so the item shows none, as the weapon import does.
    if !subclass.description.trim().is_empty() {
        recipe.flavor = subclass.description.trim().to_owned();
    }
    recipe.overrides.lore = None;
    recipe.overrides.remove_lore = true;
    if let Some(png) = &translated.icon {
        recipe.overrides.icon_edit.imported_image = Some(diamond(png)?);
    }
    let hud_color = recipe
        .overrides
        .subclass_abilities
        .as_ref()
        .and_then(|abilities| abilities.hud_color);
    recipe.overrides.subclass_abilities = Some(crate::subclass::SubclassAbilities {
        hud_color,
        ..Default::default()
    });
    Ok(recipe)
}

/// Places every source choice on the native layout and returns where each went.
fn place(
    request: &Request<'_>,
    (donor, base): (&SubclassSummary, u32),
    translated: &Translated,
    imported: &super::super::ImportedAbility,
    super_entry: u8,
    abilities: &mut crate::subclass::SubclassAbilities,
) -> Result<Vec<Value>, String> {
    let namespace = request.recipe.namespace.as_str();
    // The Super foundation: the step that equips the private Super's source row.
    let step = translated
        .grid
        .iter()
        .find(|step| {
            step.equips
                .iter()
                .any(|equip| equip.row == translated.subclass.ability_row)
        })
        .ok_or("The source grid has no Super foundation")?;
    let foundation = Foundation {
        step,
        perk: perk(&[step], &translated.carriers, namespace)?,
        imported,
    };
    let upgrades = PATHS
        .into_iter()
        .zip(&translated.upgrades.upgrades)
        .collect::<Vec<_>>();
    let middle_upgrade = upgrades
        .iter()
        .find(|(path, _)| *path == AttunementPath::Middle)
        .map(|(_, upgrade)| *upgrade)
        .filter(|upgrade| upgrade.available);
    let mut placement = Placement {
        donor,
        base,
        namespace,
        carriers: &translated.carriers,
        grid: &translated.grid,
        groups: groups(&translated.grid),
        abilities,
        records: Vec::new(),
        used: BTreeSet::new(),
    };
    placement.supers(&foundation, middle_upgrade, super_entry)?;
    placement.grenades()?;
    placement.movement()?;
    let (melee_base, melee_upgrades) = placement.melee();
    let trees = Trees {
        super_entry,
        melee_base,
        melee_upgrades,
        columns: placement.columns(&translated.upgrades.upgrades),
        passive_nodes: translated
            .upgrades
            .passives
            .iter()
            .map(|passive| passive.choice.node)
            .collect(),
        names: placement.tree_names(),
    };
    for (index, (path, upgrade)) in upgrades.into_iter().enumerate() {
        placement.tree(&trees, index, path, upgrade)?;
    }
    placement.melee_base(trees.melee_base);
    placement.automatic()?;
    placement.attributes();
    Ok(placement.records)
}

/// The private Super's source foundation, its perk and its prepared graph.
struct Foundation<'b> {
    step: &'b Step,
    perk: Option<PerkRecipe>,
    imported: &'b super::super::ImportedAbility,
}

/// What each tree reads: the middle tree's Super entry, the melee upgrades, the passive columns,
/// the passives the private bank holds and the tree names.
struct Trees<'a> {
    super_entry: u8,
    melee_base: Option<&'a Step>,
    melee_upgrades: Vec<&'a Step>,
    columns: Vec<Vec<&'a Step>>,
    passive_nodes: BTreeSet<u32>,
    names: Option<Vec<String>>,
}

/// Places the source grid on the native layout. Every source choice with a complete translation
/// takes a native entry, and every choice records where it went.
struct Placement<'a> {
    donor: &'a SubclassSummary,
    base: u32,
    namespace: &'a str,
    carriers: &'a [u16],
    grid: &'a [Step],
    groups: BTreeMap<u32, Vec<&'a Step>>,
    abilities: &'a mut crate::subclass::SubclassAbilities,
    /// Where each source choice went, for provenance.
    records: Vec<Value>,
    /// The source groups already placed.
    used: BTreeSet<u32>,
}

type Upgrade = parhelion_import::d1_roi::ability::upgrades::Upgrade;

impl<'a> Placement<'a> {
    /// The custom perk of `steps`' lowered programs, when they have any.
    fn custom_perks(&self, steps: &[&Step]) -> Result<Vec<PerkRecipe>, String> {
        Ok(perk(steps, self.carriers, self.namespace)?
            .into_iter()
            .collect())
    }

    /// A Super entry's edits: the foundation's text, icon, perk and private Super, and with
    /// `extra` the keys and description of a Super upgrade the entry also carries.
    fn super_edits(
        &self,
        foundation: &Foundation<'_>,
        extra: Option<&Upgrade>,
        entry: u8,
    ) -> Result<EntryEdits, String> {
        let step = foundation.step;
        let (removed_perks, removed_modifiers) = removed(self.donor, entry);
        let mut description = step.description.clone();
        if let Some(upgrade) = extra {
            description.push_str(&format!(
                "\n\n{}: {}",
                upgrade.choice.name.trim(),
                upgrade.choice.description.trim()
            ));
        }
        Ok(EntryEdits {
            name: Some(step.name.clone()),
            description: Some(description),
            icon: icon(step)?,
            imported_ability: Some(foundation.imported.clone()),
            // Both Super entries carry the foundation's perk, each under an identity of its own.
            custom_perks: foundation
                .perk
                .clone()
                .map(|mut perk| {
                    perk.id = format!("{}-{entry:02x}", perk.id);
                    perk
                })
                .into_iter()
                .collect(),
            modifiers: extra
                .into_iter()
                .flat_map(|upgrade| &upgrade.keys)
                .map(|&key| AbilityModifier {
                    target: entry,
                    effect: ModifierEffect::Key { key },
                })
                .collect(),
            removed_perks,
            removed_modifiers,
            ..EntryEdits::default()
        })
    }

    /// Both Super entries: the base Super and the middle tree's own, which also carries the middle
    /// tree's Super upgrade when it is available.
    fn supers(
        &mut self,
        foundation: &Foundation<'_>,
        middle_upgrade: Option<&Upgrade>,
        super_entry: u8,
    ) -> Result<(), String> {
        let step = foundation.step;
        let placed = if step.unsupported.is_empty() {
            "super"
        } else {
            "super_partial"
        };
        self.records
            .push(status(step, placed, &[layout::SUPER, super_entry]));
        let middle = self.super_edits(foundation, middle_upgrade, super_entry)?;
        let own = self.super_edits(foundation, None, layout::SUPER)?;
        self.abilities.set_path_node(
            AttunementPath::Middle,
            self.base,
            SubclassPathNode {
                position: 0,
                source: self.donor.hash,
                source_path: AttunementPath::Middle,
                source_position: 0,
                edits: middle,
            },
        );
        self.abilities.set_ability(
            self.base,
            SubclassChoice {
                entry: layout::SUPER,
                source: self.donor.hash,
                source_entry: layout::SUPER,
                edits: own,
            },
        );
        self.used.insert(step.group);
        Ok(())
    }

    /// Each source grenade equips the native grenade with the same pattern.
    fn grenades(&mut self) -> Result<(), String> {
        let (donor, grid) = (self.donor, self.grid);
        let native = |graph: Option<&u32>| {
            layout::GRENADES
                .iter()
                .copied()
                .find(|entry| donor.entry_entities.get(entry) == graph)
        };
        for step in grid.iter().filter(|step| {
            step.equips
                .iter()
                .any(|equip| native(equip.native_graph.as_ref()).is_some())
        }) {
            self.used.insert(step.group);
            let Some(entry) = native(step.equips[0].native_graph.as_ref()) else {
                continue;
            };
            if !step.unsupported.is_empty() {
                self.records.push(status(step, "native", &[entry]));
                continue;
            }
            let edits = EntryEdits {
                name: Some(step.name.clone()),
                description: Some(step.description.clone()),
                icon: icon(step)?,
                hud_artwork: hud_artwork(&[step])?,
                custom_perks: self.custom_perks(&[step])?,
                ..EntryEdits::default()
            };
            self.abilities.set_ability(
                self.base,
                SubclassChoice {
                    entry,
                    source: donor.hash,
                    source_entry: entry,
                    edits,
                },
            );
            self.records.push(status(step, "grenade", &[entry]));
        }
        Ok(())
    }

    /// The foundation equipping the native movement graph, and one choice per entry from the
    /// group of upgrades to that row.
    fn movement(&mut self) -> Result<(), String> {
        let grid = self.grid;
        let graph = self.donor.entry_entities.get(&layout::MOVEMENT[0]).copied();
        let Some(base_movement) = grid.iter().find(|step| {
            step.equips
                .iter()
                .any(|equip| equip.native_graph.is_some() && equip.native_graph == graph)
        }) else {
            return Ok(());
        };
        self.used.insert(base_movement.group);
        let row = base_movement.equips[0].row;
        let choices = self
            .groups
            .values()
            .find(|steps| {
                steps.len() == layout::MOVEMENT.len()
                    && steps.iter().all(|step| upgrades_row(step, row))
            })
            .cloned()
            .unwrap_or_default();
        let mut entries = Vec::new();
        for (index, step) in choices.into_iter().enumerate() {
            self.used.insert(step.group);
            let entry = self.movement_entry(step, index);
            if !step.unsupported.is_empty() || !base_movement.unsupported.is_empty() {
                self.records.push(status(step, "native", &[entry]));
                continue;
            }
            let (removed_perks, removed_modifiers) = removed(self.donor, entry);
            let edits = EntryEdits {
                name: Some(step.name.clone()),
                description: Some(step.description.clone()),
                icon: icon(step)?,
                hud_artwork: hud_artwork(&[base_movement, step])?,
                custom_perks: self.custom_perks(&[base_movement, step])?,
                removed_perks,
                removed_modifiers,
                ..EntryEdits::default()
            };
            self.abilities.set_ability(
                self.base,
                SubclassChoice {
                    entry,
                    source: self.donor.hash,
                    source_entry: entry,
                    edits,
                },
            );
            entries.push(entry);
            self.records.push(status(step, "movement", &[entry]));
        }
        self.records
            .push(status(base_movement, "movement_base", &entries));
        Ok(())
    }

    /// A movement upgrade takes the entry whose stock modifier applies its key, otherwise the entry
    /// of its source row.
    fn movement_entry(&self, step: &Step, index: usize) -> u8 {
        layout::MOVEMENT
            .iter()
            .copied()
            .find(|entry| {
                self.donor
                    .entry_modifiers
                    .get(entry)
                    .is_some_and(|modifiers| {
                        modifiers
                            .iter()
                            .any(|&(key, _)| step.properties.iter().any(|&(own, _)| own == key))
                    })
            })
            .unwrap_or(layout::MOVEMENT[index])
    }

    /// The foundation's upgrade to the equipped melee row, and the group of one upgrade to it per
    /// tree.
    fn melee(&mut self) -> (Option<&'a Step>, Vec<&'a Step>) {
        let grid = self.grid;
        let Some(row) = grid
            .iter()
            .filter(|step| step.foundation == Some(true))
            .flat_map(|step| &step.equips)
            .find(|equip| equip.slot == MELEE_SLOT)
            .map(|equip| equip.row)
        else {
            return (None, Vec::new());
        };
        let base = grid
            .iter()
            .find(|step| step.foundation == Some(true) && upgrades_row(step, row));
        let upgrades = if base.is_some() {
            self.groups
                .values()
                .find(|steps| {
                    steps.len() == PATHS.len()
                        && steps
                            .iter()
                            .all(|step| step.foundation != Some(true) && upgrades_row(step, row))
                })
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for step in &upgrades {
            self.used.insert(step.group);
        }
        if let Some(base) = base {
            self.used.insert(base.group);
        }
        (base, upgrades)
    }

    /// The passive columns: the remaining three-choice groups other than attributes and Super
    /// upgrades, in column order.
    fn columns(&self, upgrades: &[Upgrade]) -> Vec<Vec<&'a Step>> {
        let upgrade_nodes = upgrades
            .iter()
            .map(|upgrade| upgrade.choice.node)
            .collect::<BTreeSet<_>>();
        let mut columns = self
            .groups
            .iter()
            .filter(|(key, steps)| {
                !self.used.contains(*key)
                    && steps.len() == PATHS.len()
                    && steps.iter().all(|step| {
                        step.column >= 0
                            && step.foundation != Some(true)
                            && !upgrade_nodes.contains(&step.node)
                    })
                    && !steps.iter().all(|step| attribute(step))
            })
            .map(|(_, steps)| steps.clone())
            .collect::<Vec<_>>();
        columns.sort_by_key(|steps| steps.iter().map(|step| step.column).min());
        columns
    }

    /// Trees take the names of the first attribute column's choices, which have no behavior of
    /// their own here, in source row order.
    fn tree_names(&self) -> Option<Vec<String>> {
        let mut attributes = self
            .groups
            .values()
            .filter(|steps| steps.len() == PATHS.len() && steps.iter().all(|step| attribute(step)))
            .collect::<Vec<_>>();
        attributes.sort_by_key(|steps| steps.iter().map(|step| step.column).min());
        attributes
            .first()
            .map(|steps| steps.iter().map(|step| step.name.clone()).collect())
    }

    /// The tree of source row `index`: its name, Super upgrade, melee and passives. Without an
    /// attribute name, a tree takes its Super upgrade's name.
    fn tree(
        &mut self,
        trees: &Trees<'a>,
        index: usize,
        path: AttunementPath,
        upgrade: &Upgrade,
    ) -> Result<(), String> {
        let layout_tree = tree(path);
        let target = if path == AttunementPath::Middle {
            trees.super_entry
        } else {
            layout::SUPER
        };
        let name = trees
            .names
            .as_ref()
            .and_then(|names| names.get(index).cloned())
            .or_else(|| {
                upgrade
                    .available
                    .then(|| upgrade.choice.name.trim().to_owned())
            });
        if let Some(name) = name {
            self.abilities.set_path_name(path, self.base, Some(name));
        }
        self.super_upgrade(&layout_tree, path, upgrade, target, trees.super_entry)?;
        let melee_keys = self.tree_melee(
            &layout_tree,
            path,
            trees.melee_base,
            trees.melee_upgrades.get(index).copied(),
        )?;
        self.passives(&layout_tree, path, (index, target), &melee_keys, trees)
    }

    /// The Super upgrade, on its own node or with the tree's own Super.
    fn super_upgrade(
        &mut self,
        layout_tree: &Tree,
        path: AttunementPath,
        upgrade: &Upgrade,
        target: u8,
        super_entry: u8,
    ) -> Result<(), String> {
        let Some(position) = layout_tree.upgrade else {
            let placed = if upgrade.available {
                "super_upgrade_with_super"
            } else {
                "native"
            };
            self.records
                .push(upgrade_record(upgrade, placed, super_entry));
            return Ok(());
        };
        let entry = path.entries()[usize::from(position)];
        if upgrade.available {
            let (removed_perks, removed_modifiers) = removed(self.donor, passive_source_entry());
            let step = self
                .grid
                .iter()
                .find(|step| step.node == upgrade.choice.node);
            let edits = EntryEdits {
                name: Some(upgrade.choice.name.trim().to_owned()),
                description: Some(upgrade.choice.description.clone()),
                icon: step.map(icon).transpose()?.flatten(),
                // The selected pool's ability records feed the runtime property buckets. Each
                // path selects only its own upgrade's property groups.
                modifiers: upgrade
                    .keys
                    .iter()
                    .map(|&key| AbilityModifier {
                        target,
                        effect: ModifierEffect::Key { key },
                    })
                    .collect(),
                removed_perks,
                removed_modifiers,
                ..EntryEdits::default()
            };
            self.abilities.set_path_node(
                path,
                self.base,
                SubclassPathNode {
                    position,
                    source: self.donor.hash,
                    source_path: PASSIVE_SOURCE.0,
                    source_position: PASSIVE_SOURCE.1,
                    edits,
                },
            );
        }
        let placed = if upgrade.available {
            "super_upgrade"
        } else {
            "native"
        };
        self.records.push(upgrade_record(upgrade, placed, entry));
        Ok(())
    }

    /// The tree's melee, the foundation's upgrade and this tree's own, and the keys it applies. A
    /// native melee keeps applying its stock keys.
    fn tree_melee(
        &mut self,
        layout_tree: &Tree,
        path: AttunementPath,
        base: Option<&Step>,
        upgrade: Option<&Step>,
    ) -> Result<BTreeSet<u32>, String> {
        let entry = path.entries()[usize::from(layout_tree.melee)];
        let stock = self
            .donor
            .entry_modifiers
            .get(&entry)
            .into_iter()
            .flatten()
            .map(|&(key, _)| key)
            .collect::<BTreeSet<_>>();
        let (Some(base), Some(step)) = (base, upgrade) else {
            return Ok(stock);
        };
        if !base.unsupported.is_empty() || !step.unsupported.is_empty() {
            self.records.push(status(step, "native", &[entry]));
            return Ok(stock);
        }
        let (removed_perks, removed_modifiers) = removed(self.donor, entry);
        let edits = EntryEdits {
            name: Some(step.name.clone()),
            description: Some(step.description.clone()),
            icon: icon(step)?,
            hud_artwork: hud_artwork(&[base, step])?,
            custom_perks: self.custom_perks(&[base, step])?,
            removed_perks,
            removed_modifiers,
            ..EntryEdits::default()
        };
        self.abilities.set_path_node(
            path,
            self.base,
            SubclassPathNode {
                position: layout_tree.melee,
                source: self.donor.hash,
                source_path: path,
                source_position: layout_tree.melee,
                edits,
            },
        );
        self.records.push(status(step, "melee", &[entry]));
        Ok(applied(&[base, step]))
    }

    /// One choice from each passive column, in the tree of its source row `index`. A choice's
    /// Super keys apply to `target`, the tree's Super entry.
    fn passives(
        &mut self,
        layout_tree: &Tree,
        path: AttunementPath,
        (index, target): (usize, u8),
        melee_keys: &BTreeSet<u32>,
        trees: &Trees<'a>,
    ) -> Result<(), String> {
        for (column, &position) in trees.columns.iter().zip(&layout_tree.passives) {
            let Some(&step) = column.get(index) else {
                continue;
            };
            let entry = path.entries()[usize::from(position)];
            if !step.super_keys.is_empty() && !trees.passive_nodes.contains(&step.node) {
                return Err(format!(
                    "{} grants Super properties the private bank does not hold",
                    step.name
                ));
            }
            if !step.unsupported.is_empty() {
                self.records.push(status(step, "native", &[entry]));
                continue;
            }
            // A gated row works only beside the key it is gated on, which the tree's melee
            // applies.
            let unapplied = gates(step)
                .into_iter()
                .filter(|gate| !melee_keys.contains(gate))
                .collect::<Vec<_>>();
            if !unapplied.is_empty() {
                let mut record = status(step, "native", &[entry]);
                record["unapplied_gates"] = json!(unapplied);
                self.records.push(record);
                continue;
            }
            self.passive(step, path, position, target)?;
            self.records.push(status(step, "passive", &[entry]));
        }
        Ok(())
    }

    /// Places passive `step` at `position` of `path`, in the qualified passive pool.
    fn passive(
        &mut self,
        step: &Step,
        path: AttunementPath,
        position: u8,
        target: u8,
    ) -> Result<(), String> {
        let (removed_perks, removed_modifiers) = removed(self.donor, passive_source_entry());
        let edits = EntryEdits {
            name: Some(step.name.clone()),
            description: Some(step.description.clone()),
            icon: icon(step)?,
            custom_perks: self.custom_perks(&[step])?,
            modifiers: step
                .super_keys
                .iter()
                .map(|&key| AbilityModifier {
                    target,
                    effect: ModifierEffect::Key { key },
                })
                .collect(),
            removed_perks,
            removed_modifiers,
            ..EntryEdits::default()
        };
        self.abilities.set_path_node(
            path,
            self.base,
            SubclassPathNode {
                position,
                source: self.donor.hash,
                source_path: PASSIVE_SOURCE.0,
                source_position: PASSIVE_SOURCE.1,
                edits,
            },
        );
        Ok(())
    }

    /// Where the foundation's melee upgrade went: with each tree's melee that took its own, or
    /// native when none did.
    fn melee_base(&mut self, base: Option<&Step>) {
        let Some(base) = base else {
            return;
        };
        let entries = PATHS
            .into_iter()
            .map(|path| path.entries()[usize::from(tree(path).melee)])
            .filter(|entry| {
                self.records
                    .iter()
                    .any(|record| record["placement"] == "melee" && record["entries"][0] == *entry)
            })
            .collect::<Vec<_>>();
        let placed = if entries.is_empty() {
            "native"
        } else {
            "melee_base"
        };
        self.records.push(status(base, placed, &entries));
    }

    /// Automatic contributions outside the grid's visible choices. The class base melee and the
    /// base movement stay native, as Shadowkeep's own subclasses keep them. Programs of an
    /// automatic step without an equip go on the always-selected passives entry.
    fn automatic(&mut self) -> Result<(), String> {
        let grid = self.grid;
        for step in grid.iter().filter(|step| step.column < 0) {
            if !step.equips.is_empty() {
                self.records.push(status(step, "native_foundation", &[]));
                continue;
            }
            if step.programs.is_empty() && step.unsupported.is_empty() && equivalents(step) {
                // Every native subclass's stat passives already grant the matched actions.
                self.records
                    .push(status(step, "native_equivalent", &[layout::STAT_PASSIVES]));
                continue;
            }
            if step.programs.is_empty() || !step.unsupported.is_empty() {
                self.records
                    .push(status(step, "native", &[layout::STAT_PASSIVES]));
                continue;
            }
            let edits = EntryEdits {
                custom_perks: self.custom_perks(&[step])?,
                ..EntryEdits::default()
            };
            self.abilities.set_ability(
                self.base,
                SubclassChoice {
                    entry: layout::STAT_PASSIVES,
                    source: self.donor.hash,
                    source_entry: layout::STAT_PASSIVES,
                    edits,
                },
            );
            self.records
                .push(status(step, "foundation", &[layout::STAT_PASSIVES]));
        }
        Ok(())
    }

    /// Source attribute choices, which have no Shadowkeep counterpart and are left out.
    fn attributes(&mut self) {
        let grid = self.grid;
        for step in grid.iter().filter(|step| attribute(step)) {
            self.records.push(status(step, "left_out_attribute", &[]));
        }
    }
}

/// Where a Super upgrade went, with its keys, whether it revives and the effect graphs it selects.
fn upgrade_record(upgrade: &Upgrade, placed: &str, entry: u8) -> Value {
    json!({"node":upgrade.choice.node,"step":upgrade.choice.step,
    "name":upgrade.choice.name.trim(),"placement":placed,"entries":[entry],
    "keys":upgrade.keys,"revival":upgrade.revival,"resources":upgrade.resources})
}
