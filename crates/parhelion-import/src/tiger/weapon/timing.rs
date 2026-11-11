//! Source firing and reload timing written into the native base's stat translator.
//!
//! A weapon's stat translator (binding `2F981564`) holds one table per translation group. Each
//! table translates stat values through sampled curves and routes them to component inputs. The
//! translation matches firing and reload destinations and their operand roles, never
//! translation-column ordinals. Each scope copies the source table's curve samples into the
//! native curves its destinations read, so the source stat values the recipe carries give the
//! source's timing.
//!
//! The source and native tables share their shape in every supported version. A [`Dialect`]
//! names where each version keeps its arrays, how wide its rows are and how it numbers translated
//! outputs. The modern dialect adds a name hash and a pad word after each destination and after
//! each operand's read bytes. Those must hold the empty name and zero, since a named reference
//! has no Shadowkeep counterpart.
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};

/// The empty FNV-1 name.
const EMPTY_NAME: u32 = 0x811C_9DC5;
/// A curve sample row's cells.
const CELLS: u32 = 0x8080_000F;
/// The weapon component binding of the stat translator.
const TRANSLATOR: u32 = 0x2F98_1564;

/// Where one version keeps its stat translator arrays. Offsets are from the translator
/// definition, the owner's `+18` relative pointer, or from the row named.
pub(crate) struct Dialect {
    /// Groups, 48-byte rows of translations `+00`, copies `+10` and logic `+20`.
    groups: (usize, u32),
    /// The shared logic word, which must be zero.
    shared: usize,
    /// Stat overrides for every key, 8-byte rows.
    overrides: (usize, u32),
    /// Keys, 40-byte rows with their overrides at `+00` and the key at `+10`.
    keys: (usize, u32),
    /// Translation rows, 56 bytes.
    translations: u32,
    /// Copy rows: stride and class.
    copies: (usize, u32),
    /// The copy row's operation word and first operand.
    copy_operation: usize,
    copy_operands: usize,
    /// Logic rows, 40 bytes, with outputs `+00`, operands `+10` and kind `+20`.
    logic: u32,
    /// Logic output destinations: stride and class.
    outputs: (usize, u32),
    /// Operands: stride and class (copy rows hold three inline).
    operands: (usize, u32),
    /// The operand's channel and value words. Its first 12 bytes name what it reads.
    channel: usize,
    value: usize,
    /// Whether destinations and operands carry a name and pad after their native fields.
    names: bool,
    /// The channel of translated output zero.
    output_base: u32,
}

/// Rise of Iron.
pub(crate) static RISE_OF_IRON: Dialect = Dialect {
    groups: (0x60, 0x8080_2945),
    shared: 0x70,
    overrides: (0x278, 0x8080_2121),
    keys: (0x288, 0x8080_20A3),
    translations: 0x8080_2A32,
    copies: (80, 0x8080_2013),
    copy_operation: 0x0C,
    copy_operands: 0x10,
    logic: 0x8080_201E,
    outputs: (12, 0x8080_232B),
    operands: (20, 0x8080_23F1),
    channel: 0x0C,
    value: 0x10,
    names: false,
    output_base: 0x1E00,
};

/// Shadowkeep, the target.
pub(crate) static SHADOWKEEP: Dialect = Dialect {
    groups: (0x48, 0x8080_3975),
    shared: 0x58,
    overrides: (0x228, 0x8080_38C9),
    keys: (0x238, 0x8080_38C7),
    translations: 0x8080_3979,
    copies: (80, 0x8080_3981),
    copy_operation: 0x0C,
    copy_operands: 0x10,
    logic: 0x8080_397C,
    outputs: (12, 0x8080_3987),
    operands: (20, 0x8080_3986),
    channel: 0x0C,
    value: 0x10,
    names: false,
    output_base: 0x1E00,
};

/// Monument of Triumph (header version 53). The twin survey of 798 weapons present in both
/// versions establishes this layout and finds every name empty and every pad zero.
pub(crate) static MONUMENT_OF_TRIUMPH: Dialect = Dialect {
    groups: (0x48, 0x8080_2BDB),
    shared: 0x58,
    overrides: (0x260, 0x8080_2AD8),
    keys: (0x270, 0x8080_2AD6),
    translations: 0x8080_2BDF,
    copies: (112, 0x8080_2BE7),
    copy_operation: 0x14,
    copy_operands: 0x18,
    logic: 0x8080_2BE2,
    outputs: (20, 0x8080_2BED),
    operands: (28, 0x8080_2BEC),
    channel: 0x14,
    value: 0x18,
    names: true,
    output_base: 0xFFFF_FC00,
};

impl Dialect {
    /// Whether a translator owner's definition has this version's table shape.
    pub(crate) fn holds_table(&self, p: &Payload) -> bool {
        p.pointer(24).is_ok_and(|d| {
            p.array(d + self.groups.0, 48, Some(self.groups.1))
                .is_ok_and(|rows| !rows.is_empty())
                && p.array(d + self.keys.0, 40, Some(self.keys.1)).is_ok()
        })
    }

    /// The name and pad after a destination or an operand's read bytes must be empty.
    fn unnamed(&self, p: &Payload, at: usize) -> Result<bool> {
        Ok(!self.names || (p.u32(at)? == EMPTY_NAME && p.u32(at + 4)? == 0))
    }

    fn input(&self, p: &Payload, at: usize) -> Result<Input> {
        ensure!(
            self.unnamed(p, at + 12)?,
            "Firing destination names a source input"
        );
        Ok((p.u32(at)?, p.u32(at + 4)?, p.u32(at + 8)?))
    }

    fn operand(&self, p: &Payload, at: usize) -> Result<Operand> {
        ensure!(
            p.bytes::<12>(at)? == [0; 12] && self.unnamed(p, at + 12)?,
            "Firing operand reads another component or slot"
        );
        let channel = p.u32(at + self.channel)?;
        Ok(if channel == 0 {
            p.f32(at + self.value)?;
            Operand::Constant(p.u32(at + self.value)?)
        } else {
            ensure!(
                channel >= self.output_base && p.u32(at + self.value)? == 0x7F7FFFFF,
                "Firing operand has an unsupported output encoding"
            );
            Operand::Output(channel)
        })
    }

    /// The translated output an operand reads, or zero for a literal or another reference.
    fn channel(&self, p: &Payload, at: usize) -> Result<u32> {
        Ok(
            if p.u32(at)? == 0 && p.u32(at + self.channel)? >= self.output_base {
                p.u32(at + self.channel)?
            } else {
                0
            },
        )
    }

    /// The operand positions of a copy row.
    fn copy_operand_positions(&self, row: usize) -> [usize; 3] {
        [0, 1, 2].map(|k| row + self.copy_operands + k * self.operands.0)
    }
}

/// Component, input and selector.
type Input = (u32, u32, u32);

#[derive(Clone, Debug)]
enum Operand {
    Constant(u32),
    Output(u32),
}

#[derive(Debug)]
struct Route {
    logic: bool,
    operands: Vec<Operand>,
    /// Where each operand sits in its table's payload.
    at: Vec<usize>,
}

struct Table {
    dialect: &'static Dialect,
    payload: Payload,
    translations: Vec<usize>,
    routes: BTreeMap<Input, Vec<Route>>,
    overrides: Vec<[u8; 8]>,
    key: u32,
    index: usize,
    /// Samples a route outside the scope reads, with the outputs that read them.
    protected: BTreeMap<usize, BTreeSet<u32>>,
    /// Samples only unselected tables read, kept apart for the refusal's reason.
    unselected: BTreeSet<usize>,
    /// The outputs each destination outside the scope reads, in operand order.
    others: BTreeMap<Input, Vec<u32>>,
}

fn firing((component, input, _): Input) -> bool {
    matches!((component, input), (2, 0..=5 | 22..=25) | (4, 0..=1))
}

/// Reload Time 2, Reload Time and Empty Reload Time, the seconds the reload state takes.
fn reload((component, input, _): Input) -> bool {
    component == 3 && input <= 2
}

/// The destinations one translation pass owns.
#[derive(Clone, Copy)]
struct Scope {
    name: &'static str,
    accepts: fn(Input) -> bool,
    /// Destinations both tables must write.
    required: &'static [Input],
    /// Whether a logic row may write the scope, with the three-operand interval contract.
    interval: bool,
}

const FIRING: Scope = Scope {
    name: "Firing",
    accepts: firing,
    required: &[
        (2, 0, u32::MAX),
        (2, 1, u32::MAX),
        (2, 2, u32::MAX),
        (2, 3, u32::MAX),
        (2, 4, u32::MAX),
        (2, 5, u32::MAX),
    ],
    interval: true,
};

const RELOAD: Scope = Scope {
    name: "Reload",
    accepts: reload,
    required: &[(3, 1, u32::MAX), (3, 2, u32::MAX)],
    interval: false,
};

/// Several rows can write one destination in turn. Both tables must list the same writers in
/// the same order, which the matching compares pair by pair.
fn insert(routes: &mut BTreeMap<Input, Vec<Route>>, dest: Input, route: Route) {
    routes.entry(dest).or_default().push(route);
}

/// Copy rows write one operand. Firing destinations become routes.
fn copies(
    payload: &Payload,
    table: usize,
    (dialect, scope): (&Dialect, Scope),
    routes: &mut BTreeMap<Input, Vec<Route>>,
    others: &mut BTreeMap<Input, Vec<u32>>,
) -> Result<()> {
    for r in payload.array(table + 16, dialect.copies.0, Some(dialect.copies.1))? {
        let dest = dialect.input(payload, r)?;
        let operands = dialect.copy_operand_positions(r);
        if !(scope.accepts)(dest) {
            let read = operands
                .into_iter()
                .map(|at| dialect.channel(payload, at))
                .collect::<Result<Vec<_>>>()?;
            others.entry(dest).or_default().extend(read);
            continue;
        }
        ensure!(
            payload.u32(r + dialect.copy_operation)? == 4,
            "{} destination needs a non-copy operation",
            scope.name
        );
        insert(
            routes,
            dest,
            Route {
                logic: false,
                operands: vec![dialect.operand(payload, operands[0])?],
                at: vec![operands[0]],
            },
        );
    }
    Ok(())
}

/// Logic rows compute their outputs from several operands.
fn logic(
    payload: &Payload,
    table: usize,
    (dialect, scope): (&Dialect, Scope),
    routes: &mut BTreeMap<Input, Vec<Route>>,
    others: &mut BTreeMap<Input, Vec<u32>>,
) -> Result<()> {
    for r in payload.array(table + 32, 40, Some(dialect.logic))? {
        let outputs = payload
            .array(r, dialect.outputs.0, Some(dialect.outputs.1))?
            .into_iter()
            .map(|o| dialect.input(payload, o))
            .collect::<Result<Vec<_>>>()?;
        if !outputs.iter().any(|&o| (scope.accepts)(o)) {
            let read = payload
                .array(r + 16, dialect.operands.0, None)?
                .into_iter()
                .map(|at| dialect.channel(payload, at))
                .collect::<Result<Vec<_>>>()?;
            for dest in outputs {
                others.entry(dest).or_default().extend(read.iter().copied());
            }
            continue;
        }
        // Source 7AEE60 and native E82CC0 implement this same three-operand
        // interval calculation. Other logic kinds need their own contract.
        ensure!(
            scope.interval
                && payload.u32(r + 32)? == 0
                && outputs.iter().all(|&o| (scope.accepts)(o)),
            "{} destination needs another logic contract",
            scope.name
        );
        let at = payload.array(r + 16, dialect.operands.0, Some(dialect.operands.1))?;
        let operands = at
            .iter()
            .map(|&o| dialect.operand(payload, o))
            .collect::<Result<Vec<_>>>()?;
        ensure!(operands.len() == 3, "Firing interval operand count differs");
        for dest in outputs {
            insert(
                routes,
                dest,
                Route {
                    logic: true,
                    operands: operands.clone(),
                    at: at.clone(),
                },
            );
        }
    }
    Ok(())
}

/// The stat overrides of every table, then those of the selected key.
fn stat_overrides(payload: &Payload, at: usize, class: u32) -> Result<Vec<[u8; 8]>> {
    payload
        .array(at, 8, Some(class))?
        .into_iter()
        .map(|r| payload.bytes::<8>(r))
        .collect()
}

impl Table {
    fn read(
        payload: Payload,
        key: u32,
        (dialect, scope): (&'static Dialect, Scope),
    ) -> Result<Self> {
        let d = payload.pointer(24)?;
        let groups = payload.array(d + dialect.groups.0, 48, Some(dialect.groups.1))?;
        let keys = payload.array(d + dialect.keys.0, 40, Some(dialect.keys.1))?;
        ensure!(
            !groups.is_empty() && groups.len() == keys.len(),
            "Firing group and selector populations differ"
        );
        ensure!(
            payload.u64(d + dialect.shared)? == 0,
            "Shared firing logic needs translation"
        );
        let matches = keys
            .iter()
            .enumerate()
            .filter_map(|(i, &r)| (payload.u32(r + 16).ok() == Some(key)).then_some(i))
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "Ambiguous firing translation key");
        // Both original evaluators use table zero for an unlisted translation key.
        let index = matches.first().copied().unwrap_or(0);
        let table = groups[index];
        let mut overrides = stat_overrides(&payload, d + dialect.overrides.0, dialect.overrides.1)?;
        overrides.extend(stat_overrides(&payload, keys[index], dialect.overrides.1)?);
        let translations = payload.array(table, 56, Some(dialect.translations))?;
        let mut routes = BTreeMap::new();
        let mut others = BTreeMap::new();
        copies(&payload, table, (dialect, scope), &mut routes, &mut others)?;
        logic(&payload, table, (dialect, scope), &mut routes, &mut others)?;
        ensure!(
            scope.required.iter().all(|dest| routes.contains_key(dest)),
            "Incomplete {} timing destinations",
            scope.name.to_lowercase()
        );
        let mut result = Self {
            dialect,
            payload,
            translations,
            routes,
            overrides,
            key,
            index,
            protected: BTreeMap::new(),
            unselected: BTreeSet::new(),
            others,
        };
        let foreign = result
            .others
            .values()
            .flatten()
            .copied()
            .filter(|&channel| channel != 0)
            .collect::<BTreeSet<_>>();
        for channel in foreign {
            for cell in result.curve(channel)?.1 {
                result.protected.entry(cell).or_default().insert(channel);
            }
        }
        result.protect_unselected(&groups, table)?;
        Ok(result)
    }

    /// A private owner can still share serialized sample storage between modes.
    /// Keep every unselected table intact, including aliased curve descriptors.
    fn protect_unselected(&mut self, groups: &[usize], table: usize) -> Result<()> {
        for &other in groups.iter().filter(|&&row| row != table) {
            for translation in self.payload.array(other, 56, None)? {
                for sample in self.payload.array(translation + 16, 16, None)? {
                    self.unselected
                        .extend(self.payload.array(sample, 4, Some(CELLS))?);
                }
            }
        }
        Ok(())
    }

    fn curve(&self, channel: u32) -> Result<(usize, Vec<usize>)> {
        let index = usize::try_from(
            channel
                .checked_sub(self.dialect.output_base)
                .context("Firing output number")?,
        )?;
        let row = *self
            .translations
            .get(index / 32)
            .context("Firing output exceeds its translations")?;
        let samples = self.payload.array(row + 16, 16, None)?;
        ensure!(!samples.is_empty(), "Empty firing curve");
        let cells = samples
            .into_iter()
            .map(|sample| {
                let cells = self.payload.array(sample, 4, Some(CELLS))?;
                let at = *cells
                    .get(index % 32)
                    .context("Firing column exceeds its outputs")?;
                self.payload.f32(at)?;
                Ok(at)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((row, cells))
    }
}

/// A source stat translator and the translation group its weapon selects a table by.
pub(crate) struct Translator {
    pub owner: u32,
    pub payload: Payload,
    pub key: u32,
    pub dialect: &'static Dialect,
}

/// The investment stat definition behind each Weapon Stats input. The investment constants blob
/// lists one per input, with an extra entry after input 5. The stock routes place Reload Speed at
/// input 9, Handling at 14, Aim Assistance at 17 and Recoil Direction at 18, so its entries from
/// the seventh on belong one input earlier. Input 2 has none.
const STAT_INPUTS: [Option<u16>; 19] = [
    Some(14),
    Some(24),
    None,
    Some(32),
    Some(17),
    Some(22),
    Some(20),
    Some(16),
    Some(21),
    Some(27),
    Some(18),
    Some(15),
    None,
    Some(25),
    Some(26),
    Some(26),
    Some(26),
    Some(30),
    Some(31),
];

/// The source's carried stat values by Weapon Stats input, for routes that take a literal. Rows
/// are investment stat hashes with their values.
fn stat_inputs(native: &mut Reader, rows: &[(u32, f64)]) -> Result<BTreeMap<u32, f32>> {
    let definitions = crate::tiger::shadowkeep::stat_definitions(native)?;
    let mut stats = BTreeMap::new();
    for &(hash, value) in rows {
        let input = definitions.get(&hash).and_then(|&index| {
            STAT_INPUTS
                .iter()
                .position(|&definition| definition == Some(index))
        });
        if let Some(input) = input {
            stats.insert(u32::try_from(input)?, value as f32);
        }
    }
    Ok(stats)
}

/// Translate the source's firing and reload timing onto the donor's native stat translator.
/// Firing and reload are separate passes over the same tables. Either can stand alone, and a
/// refusal of one leaves the other's curves in place and says why. The report's `edits` are
/// four-byte patches relative to the translator resource, which the package build applies to a
/// private owner after checking the native owner's digest, resource offset and key. `stats` are
/// the source's carried investment stat values by stat hash.
pub(crate) fn translate(
    source: &Translator,
    native: &mut Reader,
    donor: u32,
    stats: &[(u32, f64)],
) -> Result<Value> {
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, TRANSLATOR).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1,
        "Native firing translator binding differs"
    );
    let binding = &bindings[0];
    let native_payload = (*native.tag(binding.owner_tag, Some(0x80809C36))?).clone();
    let tables = |scope: Scope| -> Result<(Table, Table)> {
        Ok((
            Table::read(source.payload.clone(), source.key, (source.dialect, scope))?,
            Table::read(
                native_payload.clone(),
                runtime.weapon_translation_group_hash,
                (&SHADOWKEEP, scope),
            )?,
        ))
    };
    let stats = stat_inputs(native, stats)?;
    let mut cells = BTreeMap::new();
    let mut selected = None;
    let mut passes = BTreeMap::new();
    for scope in [FIRING, RELOAD] {
        let mut scoped = cells.clone();
        let result = tables(scope).and_then(|(from, to)| {
            let curves = matched(&from, &to, scope, &stats, &mut scoped)?;
            Ok(((from.key, from.index, to.key, to.index), curves))
        });
        let pass = match result {
            Ok((tables, curves)) => {
                cells = scoped;
                selected.get_or_insert(tables);
                json!({"status":"translated","curves":curves})
            }
            Err(error) => {
                crate::cancellation::check()?;
                json!({"status":"native","reason":format!("{error:#}")})
            }
        };
        passes.insert(scope.name, pass);
    }
    let Some((source_key, source_index, native_key, native_index)) = selected else {
        anyhow::bail!(
            "{}",
            passes[FIRING.name]["reason"]
                .as_str()
                .unwrap_or("Source timing is not translated")
        );
    };
    let edits = cells.into_iter().filter(|(at, bytes)| native_payload.0.get(*at..at + 4) != Some(bytes.as_slice())).map(|(at, bytes)| {
        let offset = u64::try_from(at)?.checked_sub(binding.resource_offset).context("Timing patch precedes its binding")?;
        Ok(json!({"offset":u32::try_from(offset)?,"before":native_payload.bytes::<4>(at)?,"after":bytes}))
    }).collect::<Result<Vec<_>>>()?;
    let firing = &passes[FIRING.name];
    let report = json!({"status":"translated","source_owner":source.owner,"native_owner":binding.owner_tag,
        "native_resource":binding.resource_offset,"native_sha256":hex::encode(Sha256::digest(&native_payload.0)),
        "source_key":source_key,"native_key":native_key,"source_table":source_index,"native_table":native_index,
        "curves":firing.get("curves").cloned().unwrap_or_else(|| json!([])),
        "firing":{"status":firing["status"],"reason":firing.get("reason")},
        "reload":passes[RELOAD.name],"edits":edits,
        "scope":"Rate, burst size, interval, charge and reload time inputs, each pass on its own. Other stat curves and runtime state machines remain native.","gameplay_verified":false});
    Ok(report)
}

/// Copies the source curve samples behind every destination of `scope` into `cells`, by native
/// sample offset, and returns the curves it read. Both tables must write the same destinations
/// with the same operations, and a native sample another input or mode reads must already hold
/// the source value.
fn matched(
    from: &Table,
    to: &Table,
    scope: Scope,
    stats: &BTreeMap<u32, f32>,
    cells: &mut BTreeMap<usize, [u8; 4]>,
) -> Result<Vec<Value>> {
    let channels = correspondence(from, to, scope)?;
    let sharing = Sharing::read(from, to, scope, &channels);
    let mut curves = Vec::new();
    for (n, s) in channels {
        let (sr, sc) = from.curve(s)?;
        let (nr, nc) = to.curve(n)?;
        ensure!(
            from.payload.bytes::<24>(sr + 32)? == to.payload.bytes::<24>(nr + 32)?
                && sc.len() == nc.len(),
            "{} stat selector, interpolation or domain differs",
            scope.name
        );
        let stat = from.payload.u32(sr + 32)?;
        ensure!(
            stat_overrides_of(&from.overrides, stat) == stat_overrides_of(&to.overrides, stat),
            "{} stat overrides differ",
            scope.name
        );
        if let Some(reason) = sharing.kept(from, to, scope, n, (&sc, &nc))? {
            let value = literal(from, sr, &sc, stat, stats).with_context(|| reason.clone())?;
            write_literal(to, scope, n, value, cells)?;
            curves.push(
                json!({"stat_input":stat,"source_channel":s,"native_channel":n,
                "literal":value,"reason":reason}),
            );
            continue;
        }
        let values = copy_samples(from, scope, (sc, nc), cells)?;
        curves.push(
            json!({"stat_input":stat,"source_channel":s,"native_channel":n,"samples":values,
                "shared_inputs":sharing.shared.get(&n)}),
        );
    }
    ensure!(
        !curves.is_empty(),
        "No {} curves were qualified",
        scope.name.to_lowercase()
    );
    Ok(curves)
}

/// The source output behind each native output of the scope. Both tables must write the same
/// destinations with the same writers in the same order, the same operations and the same
/// constants.
fn correspondence(from: &Table, to: &Table, scope: Scope) -> Result<BTreeMap<u32, u32>> {
    ensure!(
        from.routes.keys().eq(to.routes.keys()),
        "Source and native {} destinations differ",
        scope.name.to_lowercase()
    );
    let mut channels = BTreeMap::new();
    for (dest, source_routes) in &from.routes {
        let targets = &to.routes[dest];
        ensure!(
            source_routes.len() == targets.len(),
            "Source and native {} writer counts differ",
            scope.name.to_lowercase()
        );
        for (source_route, target) in source_routes.iter().zip(targets) {
            ensure!(
                source_route.logic == target.logic
                    && source_route.operands.len() == target.operands.len(),
                "Source and native {} operations differ",
                scope.name.to_lowercase()
            );
            for (s, n) in source_route.operands.iter().zip(&target.operands) {
                match (s, n) {
                    (Operand::Constant(a), Operand::Constant(b)) => {
                        ensure!(a == b, "{} logic constants differ", scope.name)
                    }
                    (Operand::Output(a), Operand::Output(b)) => {
                        ensure!(
                            channels.insert(*b, *a).is_none_or(|old| old == *a),
                            "Conflicting {} output correspondence",
                            scope.name.to_lowercase()
                        );
                    }
                    _ => anyhow::bail!("{} operand kinds differ", scope.name),
                }
            }
        }
    }
    Ok(channels)
}

/// The stat overrides that name `stat`.
fn stat_overrides_of(rows: &[[u8; 8]], stat: u32) -> Vec<[u8; 8]> {
    rows.iter()
        .filter(|r| u32::from_le_bytes([r[0], r[1], r[2], r[3]]) == stat)
        .copied()
        .collect()
}

/// Native outputs of the scope that also drive inputs outside it.
///
/// Such an output can take the source curve only when the source drives that input from the
/// corresponding output too. The change then gives that input the source's value as well, as the
/// source table does. Otherwise the scope's routes read the source value as a literal and the
/// shared output stays.
struct Sharing {
    /// Outputs the source drives the same way, with the inputs they also drive.
    shared: BTreeMap<u32, Vec<Value>>,
    /// Outputs the source drives another way, with the reason.
    conflicts: BTreeMap<u32, String>,
}

impl Sharing {
    fn read(from: &Table, to: &Table, scope: Scope, channels: &BTreeMap<u32, u32>) -> Self {
        let mut shared = BTreeMap::<u32, Vec<Value>>::new();
        let mut conflicts = BTreeMap::<u32, String>::new();
        for (dest, read) in &to.others {
            for (position, native_channel) in read.iter().enumerate() {
                let Some(&source_channel) = channels.get(native_channel) else {
                    continue;
                };
                if from.others.get(dest).and_then(|r| r.get(position)) == Some(&source_channel) {
                    shared
                        .entry(*native_channel)
                        .or_default()
                        .push(json!({"component":dest.0,"input":dest.1}));
                } else {
                    conflicts.entry(*native_channel).or_insert_with(|| {
                        format!(
                            "{} output {native_channel:X} also drives component {} input {}, which the source drives another way",
                            scope.name, dest.0, dest.1
                        )
                    });
                }
            }
        }
        Self { shared, conflicts }
    }

    /// Why native output `n` must keep its curve and read a literal instead, if it must: another
    /// input drives it another way, another output of the table shares its storage, or another
    /// weapon's table shares samples the source would change.
    fn kept(
        &self,
        from: &Table,
        to: &Table,
        scope: Scope,
        n: u32,
        (sc, nc): (&[usize], &[usize]),
    ) -> Result<Option<String>> {
        let storage = nc.iter().any(|cell| {
            to.protected
                .get(cell)
                .is_some_and(|readers| readers.iter().any(|c| !self.shared.contains_key(c)))
        });
        let mut unselected = false;
        for (&source_cell, cell) in sc.iter().zip(nc) {
            unselected |= to.unselected.contains(cell)
                && from.payload.bytes::<4>(source_cell)? != to.payload.bytes::<4>(*cell)?;
        }
        let reason = self.conflicts.get(&n).cloned().or_else(|| {
            storage.then(|| {
                format!(
                    "{} curve storage is shared with another output of the weapon's table",
                    scope.name
                )
            })
        });
        Ok(reason.or_else(|| {
            unselected
                .then(|| format!("{} curve is shared with another weapon's table", scope.name))
        }))
    }
}

/// Every native operand of the scope that reads output `n` takes `value` as its constant.
fn write_literal(
    to: &Table,
    scope: Scope,
    n: u32,
    value: f32,
    cells: &mut BTreeMap<usize, [u8; 4]>,
) -> Result<()> {
    for route in to.routes.values().flatten() {
        for (operand, &at) in route.operands.iter().zip(&route.at) {
            if !matches!(operand, Operand::Output(c) if *c == n) {
                continue;
            }
            for (cell, bytes) in [
                (at + to.dialect.channel, [0; 4]),
                (at + to.dialect.value, value.to_le_bytes()),
            ] {
                ensure!(
                    cells.insert(cell, bytes).is_none_or(|old| old == bytes),
                    "Literal {} operand disagrees",
                    scope.name.to_lowercase()
                );
            }
        }
    }
    Ok(())
}

/// The source samples written over the native samples, which returns the values copied.
fn copy_samples(
    from: &Table,
    scope: Scope,
    (sc, nc): (Vec<usize>, Vec<usize>),
    cells: &mut BTreeMap<usize, [u8; 4]>,
) -> Result<Vec<f32>> {
    let mut values = Vec::new();
    for (source_cell, target_cell) in sc.into_iter().zip(nc) {
        let bytes = from.payload.bytes::<4>(source_cell)?;
        ensure!(
            cells
                .insert(target_cell, bytes)
                .is_none_or(|old| old == bytes),
            "Aliased {} curves disagree",
            scope.name.to_lowercase()
        );
        values.push(from.payload.f32(source_cell)?);
    }
    Ok(values)
}

/// The source curve at the source stat value its input reads, as its own evaluator samples it:
/// the value's place in the curve's range picks a tier, and an interpolating curve blends the
/// next.
fn literal(
    from: &Table,
    row: usize,
    cells: &[usize],
    stat: u32,
    stats: &BTreeMap<u32, f32>,
) -> Result<f32> {
    let value = *stats
        .get(&stat)
        .with_context(|| format!("The source carries no value for stat input {stat}"))?;
    ensure!(
        !from
            .overrides
            .iter()
            .any(|r| u32::from_le_bytes(r[..4].try_into().unwrap_or_default()) == stat),
        "The source overrides its stat input {stat}"
    );
    let p = &from.payload;
    let (low, high) = (p.f32(row + 0x28)?, p.f32(row + 0x2C)?);
    ensure!(high > low && !cells.is_empty(), "Source curve has no range");
    let samples = cells
        .iter()
        .map(|&cell| p.f32(cell))
        .collect::<Result<Vec<_>>>()?;
    let last = samples.len() - 1;
    let x = (value.clamp(low, high) - low) / (high - low) * last as f32;
    let first = (x.floor() as usize).min(last);
    let fraction = if p.u32(row + 0x30)? == 1 {
        x - first as f32
    } else {
        0.
    };
    let next = samples[(first + 1).min(last)];
    let result = samples[first] + (next - samples[first]) * fraction;
    ensure!(result.is_finite(), "Source literal is not finite");
    Ok(result)
}
