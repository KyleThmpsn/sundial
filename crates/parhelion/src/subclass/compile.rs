//! What an authored ability or path node compiles to beside its records: its custom perks and a
//! copy of its entity. Its modifiers are records of its pool and rows of stock banks, which
//! placement and the bank stage write.
//!
//! Each effect becomes a hidden private finished sandbox perk, and the entry's pool grants that
//! row. An effect that keeps its stock identity grants the stock row instead. An entry with values
//! of its own gets a copy of its stock entity with the values changed, under an ability row of its
//! own, and its pool equips that row. Recolored effects ride on the same copy. A perk in the list whose conditions name the copied
//! ability by its stock entity names the copy instead: a custom perk once compiled, and a stock
//! perk through a private copy that keeps its presentation and replaces it in its pool. The build
//! supplies the private perk machinery, which private plugs share, through [`Planner`] and
//! [`Compiler`].
use sundial::package_authoring::runtime::WeaponRuntimeValueOverride;
use tiger_pkg::TagHash;

use super::authoring::{AuthoredEntry, CompiledEntry, ResolvedList};
use super::tables::SubclassTables;
use super::{BankValue, EffectGrade, PaletteEdit, SpawnSwap, TintEdit};
use crate::error::invalid;
use crate::{AuthoringResult, WeaponSandboxPerkRuntimeOverride};

/// How the build plans private perks and abilities: identities apart from every stock perk,
/// ability and entity and every private plug's perk.
pub(crate) trait Planner {
    type Perk;
    /// The private perk `effect` compiles to, named for `role`, or `None` for an effect that
    /// keeps its stock identity.
    fn effect(
        &mut self,
        role: (&str, &str),
        effect: WeaponSandboxPerkRuntimeOverride,
    ) -> AuthoringResult<Option<Self::Perk>>;
    /// An ability row's identity and pattern hashes, named for `role`: an identity no ability row
    /// holds, and a pattern no entity assignment holds.
    fn ability(&mut self, role: (&str, &str)) -> AuthoringResult<(u32, u32)>;
    /// A private copy of stock perk `perk` that keeps its presentation, named for `role`.
    fn stock(&mut self, role: (&str, &str), perk: u16) -> AuthoringResult<Self::Perk>;
}

/// What a copy of an ability entity changes: its values, its effects' colors, the projectiles it
/// fires, values of its bank's rows, the damage type of its damage profiles and the glyph its HUD
/// tile shows. With the stock entity, it is everything the copy is made from besides the native
/// payloads the build reads, so the build keeps copies between builds by it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EntityChanges<'a> {
    pub(crate) values: &'a [WeaponRuntimeValueOverride],
    pub(crate) palettes: &'a [PaletteEdit],
    pub(crate) tints: &'a [TintEdit],
    pub(crate) grade: Option<EffectGrade>,
    pub(crate) swaps: &'a [SpawnSwap],
    pub(crate) bank_values: &'a [BankValue],
    pub(crate) damage_type: Option<u8>,
    pub(crate) hud_glyph: Option<u32>,
    /// The keys whose bank rows name a glyph, which name `hud_glyph` too.
    pub(crate) hud_keys: &'a [u32],
    pub(crate) attached: &'a [AttachedGlyph],
}

/// An attached graph's controller glyph and each stock-to-private conditional glyph mapping.
#[derive(Clone, Debug)]
pub(crate) struct AttachedGlyph {
    pub(crate) graph: u32,
    pub(crate) glyph: u32,
    pub(crate) variants: Vec<(u32, u32)>,
}

/// How the build compiles them, into the same tables as the private plugs.
pub(crate) trait Compiler {
    type Perk;
    /// Reports `operation` as build progress as it starts.
    fn report(&mut self, operation: &str);
    /// Compiles `perk` with each ability key that `moves` names moved from the stock entity to
    /// its copy. Returns its finished sandbox-perk row.
    fn perk(&mut self, perk: &Self::Perk, moves: &[(u32, u32)]) -> AuthoringResult<u16>;
    /// Compiles `perk`, a private copy of a stock perk that names a moved ability, as `perk`
    /// does. Refuses one that names none.
    fn retargeted(&mut self, perk: &Self::Perk, moves: &[(u32, u32)]) -> AuthoringResult<u16>;
    /// Copies the entity of `entry`, its label and stock entity, with `changes`, assigns the copy
    /// to `pattern`, and returns the copy.
    fn entity(
        &mut self,
        entry: (&str, TagHash),
        changes: EntityChanges<'_>,
        pattern: u32,
    ) -> AuthoringResult<TagHash>;
}

/// One authored entry's planned perks, entity, and private copies of stock perks.
pub(crate) struct EntryPlan<P> {
    /// The entry's place among its list's authored entries.
    entry: usize,
    effects: Vec<Effect<P>>,
    entity: Option<EntityPlan>,
    /// Each stock perk that names a copied ability, and its private copy.
    retargeted: Vec<(u16, P)>,
}

enum Effect<P> {
    Private(P),
    Stock(u16),
}

/// The stock ability row and entity an entry copies, the values and palettes the copy changes,
/// and the hashes of its own row.
struct EntityPlan {
    row: u8,
    source: TagHash,
    values: Vec<WeaponRuntimeValueOverride>,
    palettes: Vec<PaletteEdit>,
    tints: Vec<TintEdit>,
    grade: Option<EffectGrade>,
    swaps: Vec<SpawnSwap>,
    bank_values: Vec<BankValue>,
    damage_type: Option<u8>,
    hud_glyph: Option<u32>,
    hud_keys: Vec<u32>,
    attached: Vec<AttachedGlyph>,
    identity: u32,
    pattern: u32,
}

impl<P> EntryPlan<P> {
    /// The effects that compile to private perks, then the private copies of stock perks.
    pub(crate) fn private_perks(&self) -> impl Iterator<Item = &P> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Private(perk) => Some(perk),
                Effect::Stock(_) => None,
            })
            .chain(self.retargeted.iter().map(|(_, perk)| perk))
    }
}

/// Plans every authored entry of an item's list that compiles anything. An item without a list of
/// its own plans nothing.
pub(crate) fn plan<L: Planner>(
    list: Option<&ResolvedList>,
    namespace: &str,
    planner: &mut L,
) -> AuthoringResult<Vec<EntryPlan<L::Perk>>> {
    let mut plans = Vec::new();
    for (index, entry) in list.iter().flat_map(|list| list.entries.iter()).enumerate() {
        if entry.effect_count() == 0 && entry.entity.is_none() && entry.retargeted.is_empty() {
            continue;
        }
        let plan = plan_entry(entry, namespace, planner)
            .map_err(|error| error.context(entry.label.clone()))?;
        plans.push(EntryPlan {
            entry: index,
            ..plan
        });
    }
    Ok(plans)
}

fn plan_entry<L: Planner>(
    entry: &AuthoredEntry,
    namespace: &str,
    planner: &mut L,
) -> AuthoringResult<EntryPlan<L::Perk>> {
    let perks = entry
        .custom_perks
        .iter()
        .map(|perk| {
            let effects = perk
                .compiler_effects()
                .map_err(|error| invalid(format!("{}: {error}", perk.name)))?;
            Ok((perk.id.as_str(), effects))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let mut effects = Vec::new();
    for (id, compiled) in perks {
        for (number, effect) in compiled.into_iter().enumerate() {
            let stock = effect.source_perk_index;
            let role = format!("{}/perk/{id}/effect/{number}", entry.key);
            effects.push(match planner.effect((namespace, &role), effect)? {
                Some(perk) => Effect::Private(perk),
                None => Effect::Stock(stock),
            });
        }
    }
    let entity = entry
        .entity
        .as_ref()
        .map(|own| {
            let (identity, pattern) =
                planner.ability((namespace, &format!("{}/ability", entry.key)))?;
            Ok(EntityPlan {
                row: own.row,
                source: own.source,
                values: own.values.clone(),
                palettes: own.palettes.clone(),
                tints: own.tints.clone(),
                grade: own.grade,
                swaps: own.swaps.clone(),
                bank_values: own.bank_values.clone(),
                damage_type: own.damage_type,
                hud_glyph: own.hud_glyph,
                hud_keys: own.hud_keys.clone(),
                attached: own
                    .attached
                    .iter()
                    .map(|attached| AttachedGlyph {
                        graph: attached.graph,
                        glyph: attached.rows[0].key,
                        variants: attached
                            .rows
                            .iter()
                            .map(|row| (row.source, row.key))
                            .collect(),
                    })
                    .collect(),
                identity,
                pattern,
            })
        })
        .transpose()?;
    let retargeted = entry
        .retargeted
        .iter()
        .map(|&stock| {
            let role = format!("{}/stock/{stock}", entry.key);
            Ok((stock, planner.stock((namespace, &role), stock)?))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(EntryPlan {
        entry: 0,
        effects,
        entity,
        retargeted,
    })
}

/// Compiles an item's plans, adding each entity copy's ability row to `tables`. Returns what
/// placement takes for each of its authored entries, with no icons yet.
pub(crate) fn compile<C: Compiler>(
    list: Option<&ResolvedList>,
    plans: &[EntryPlan<C::Perk>],
    compiler: &mut C,
    tables: &mut SubclassTables,
) -> AuthoringResult<Vec<CompiledEntry>> {
    let Some(list) = list else {
        return Ok(Vec::new());
    };
    let mut compiled = vec![CompiledEntry::default(); list.entries.len()];
    // Entity copies first, so every perk of the list that names a copied ability can name the
    // copy.
    let mut moves = Vec::new();
    for plan in plans {
        let Some(entity) = &plan.entity else {
            continue;
        };
        let (label, entry) = slot(list, &mut compiled, plan.entry)?;
        (|| -> AuthoringResult<()> {
            let copy = compiler.entity(
                (label, entity.source),
                EntityChanges {
                    values: &entity.values,
                    palettes: &entity.palettes,
                    tints: &entity.tints,
                    grade: entity.grade,
                    swaps: &entity.swaps,
                    bank_values: &entity.bank_values,
                    damage_type: entity.damage_type,
                    hud_glyph: entity.hud_glyph,
                    hud_keys: &entity.hud_keys,
                    attached: &entity.attached,
                },
                entity.pattern,
            )?;
            entry.row = Some(tables.append_ability(entity.row, entity.identity, entity.pattern)?);
            moves.push((entity.source.0, copy.0));
            Ok(())
        })()
        .map_err(|error| error.context(label.to_owned()))?;
    }
    for plan in plans {
        let (label, entry) = slot(list, &mut compiled, plan.entry)?;
        if plan.private_perks().next().is_some() {
            compiler.report(&format!("Compiling Perks of {label}"));
        }
        (|| -> AuthoringResult<()> {
            for effect in &plan.effects {
                entry.perks.push(match effect {
                    Effect::Stock(index) => *index,
                    Effect::Private(perk) => compiler.perk(perk, &moves)?,
                });
            }
            for (stock, perk) in &plan.retargeted {
                entry
                    .retargeted
                    .push((*stock, compiler.retargeted(perk, &moves)?));
            }
            Ok(())
        })()
        .map_err(|error| error.context(label.to_owned()))?;
    }
    Ok(compiled)
}

/// An authored entry's label and what it compiles to.
fn slot<'a>(
    list: &'a ResolvedList,
    compiled: &'a mut [CompiledEntry],
    entry: usize,
) -> AuthoringResult<(&'a str, &'a mut CompiledEntry)> {
    let label = list
        .entries
        .get(entry)
        .map_or("Subclass entry", |entry| entry.label.as_str());
    let compiled = compiled
        .get_mut(entry)
        .ok_or_else(|| invalid(format!("{label} is outside its list")))?;
    Ok((label, compiled))
}

/// Gives each compiled entry its artwork's icon row.
pub(crate) fn with_icons(
    mut compiled: Vec<CompiledEntry>,
    icons: &[Option<u16>],
) -> Vec<CompiledEntry> {
    for (entry, icon) in compiled.iter_mut().zip(icons) {
        entry.icon = *icon;
    }
    compiled
}
