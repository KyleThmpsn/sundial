//! Match firing destinations and their operand roles, never translation-column ordinals.
use super::*;
use sha2::{Digest, Sha256};
use sundial::package_authoring::{entity::weapon_component_bindings, runtime};

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
}

struct Table {
    tag: u32,
    payload: Payload,
    translations: Vec<usize>,
    routes: BTreeMap<Input, Route>,
    overrides: Vec<[u8; 8]>,
    key: u32,
    index: usize,
    protected: BTreeSet<usize>,
}

fn firing((component, input, _): Input) -> bool {
    matches!((component, input), (2, 0..=5 | 22..=25) | (4, 0..=1))
}

fn input(p: &Payload, at: usize) -> Result<Input> {
    Ok((p.u32(at)?, p.u32(at + 4)?, p.u32(at + 8)?))
}

fn operand(p: &Payload, at: usize) -> Result<Operand> {
    ensure!(
        p.bytes::<12>(at)? == [0; 12],
        "Firing operand reads another component or slot"
    );
    let channel = p.u32(at + 12)?;
    Ok(if channel == 0 {
        p.f32(at + 16)?;
        Operand::Constant(p.u32(at + 16)?)
    } else {
        ensure!(
            channel >= 0x1E00 && p.u32(at + 16)? == 0x7F7FFFFF,
            "Firing operand has an unsupported output encoding"
        );
        Operand::Output(channel)
    })
}

/// A destination outside firing keeps the translated channel it reads intact.
fn protect(payload: &Payload, at: usize, channels: &mut BTreeSet<u32>) -> Result<()> {
    if payload.u32(at)? == 0 && payload.u32(at + 12)? >= 0x1E00 {
        channels.insert(payload.u32(at + 12)?);
    }
    Ok(())
}

fn insert(routes: &mut BTreeMap<Input, Route>, dest: Input, route: Route) -> Result<()> {
    ensure!(
        routes.insert(dest, route).is_none(),
        "Firing destination has multiple writers"
    );
    Ok(())
}

/// Copy rows write one operand. Firing destinations become routes.
fn copies(
    payload: &Payload,
    table: usize,
    old: bool,
    routes: &mut BTreeMap<Input, Route>,
    protected: &mut BTreeSet<u32>,
) -> Result<()> {
    for r in payload.array(
        table + 16,
        80,
        Some(if old { 0x80802013 } else { 0x80803981 }),
    )? {
        let dest = input(payload, r)?;
        if !firing(dest) {
            for at in [r + 16, r + 36, r + 56] {
                protect(payload, at, protected)?;
            }
            continue;
        }
        ensure!(
            payload.u32(r + 12)? == 4,
            "Firing destination needs a non-copy operation"
        );
        let operands = vec![operand(payload, r + 16)?];
        insert(
            routes,
            dest,
            Route {
                logic: false,
                operands,
            },
        )?;
    }
    Ok(())
}

/// Logic rows compute their outputs from several operands.
fn logic(
    payload: &Payload,
    table: usize,
    old: bool,
    routes: &mut BTreeMap<Input, Route>,
    protected: &mut BTreeSet<u32>,
) -> Result<()> {
    for r in payload.array(
        table + 32,
        40,
        Some(if old { 0x8080201E } else { 0x8080397C }),
    )? {
        let outputs = payload
            .array(r, 12, Some(if old { 0x8080232B } else { 0x80803987 }))?
            .into_iter()
            .map(|o| input(payload, o))
            .collect::<Result<Vec<_>>>()?;
        if !outputs.iter().any(|&o| firing(o)) {
            for at in payload.array(r + 16, 20, None)? {
                protect(payload, at, protected)?;
            }
            continue;
        }
        // Source 7AEE60 and native E82CC0 implement this same three-operand
        // interval calculation. Other logic kinds need their own contract.
        ensure!(
            payload.u32(r + 32)? == 0 && outputs.iter().all(|&o| firing(o)),
            "Firing destination needs another logic contract"
        );
        let operands = payload
            .array(r + 16, 20, Some(if old { 0x808023F1 } else { 0x80803986 }))?
            .into_iter()
            .map(|o| operand(payload, o))
            .collect::<Result<Vec<_>>>()?;
        ensure!(operands.len() == 3, "Firing interval operand count differs");
        for dest in outputs {
            insert(
                routes,
                dest,
                Route {
                    logic: true,
                    operands: operands.clone(),
                },
            )?;
        }
    }
    Ok(())
}

/// The stat overrides of every table, then those of the selected key.
fn stat_overrides(payload: &Payload, at: usize, old: bool) -> Result<Vec<[u8; 8]>> {
    payload
        .array(at, 8, Some(if old { 0x80802121 } else { 0x808038C9 }))?
        .into_iter()
        .map(|r| payload.bytes::<8>(r))
        .collect()
}

impl Table {
    fn read(tag: u32, payload: Payload, key: u32, old: bool) -> Result<Self> {
        let d = payload.pointer(24)?;
        let (groups, keys, shared, all) = if old {
            (0x60, 0x288, 0x70, 0x278)
        } else {
            (0x48, 0x238, 0x58, 0x228)
        };
        let groups = payload.array(
            d + groups,
            48,
            Some(if old { 0x80802945 } else { 0x80803975 }),
        )?;
        let keys = payload.array(
            d + keys,
            40,
            Some(if old { 0x808020A3 } else { 0x808038C7 }),
        )?;
        ensure!(
            !groups.is_empty() && groups.len() == keys.len(),
            "Firing group and selector populations differ"
        );
        ensure!(
            payload.u64(d + shared)? == 0,
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
        let mut overrides = stat_overrides(&payload, d + all, old)?;
        overrides.extend(stat_overrides(&payload, keys[index], old)?);
        let translations =
            payload.array(table, 56, Some(if old { 0x80802A32 } else { 0x80803979 }))?;
        let mut routes = BTreeMap::new();
        let mut protected_channels = BTreeSet::new();
        copies(&payload, table, old, &mut routes, &mut protected_channels)?;
        logic(&payload, table, old, &mut routes, &mut protected_channels)?;
        ensure!(
            (0..=5).all(|i| routes.contains_key(&(2, i, u32::MAX))),
            "Incomplete firing timing destinations"
        );
        let mut result = Self {
            tag,
            payload,
            translations,
            routes,
            overrides,
            key,
            index,
            protected: BTreeSet::new(),
        };
        for channel in protected_channels {
            result.protected.extend(result.curve(channel)?.1);
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
                    self.protected
                        .extend(self.payload.array(sample, 4, Some(0x8080000F))?);
                }
            }
        }
        Ok(())
    }

    fn curve(&self, channel: u32) -> Result<(usize, Vec<usize>)> {
        let index = usize::try_from(
            channel
                .checked_sub(0x1E00)
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
                let cells = self.payload.array(sample, 4, Some(0x8080000F))?;
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

pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    export: &Value,
    donor: u32,
) -> Result<Value> {
    let (_, patterns) = source.table(0x8080339A)?;
    let index = usize::try_from(
        export["runtime"]["pattern_index"]
            .as_u64()
            .context("Source pattern index")?,
    )?;
    let pattern = *patterns
        .array(8, 32, Some(0x808033BC))?
        .get(index)
        .context("Source firing pattern")?;
    let key = patterns.u32(pattern + 8)?;
    let root = u32::from_str_radix(
        export["runtime"]["runtime_entity"]
            .as_str()
            .context("Source firing runtime")?,
        16,
    )?;
    let entity = source.read(root)?;
    let mut candidates = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(r)?;
        let p = source.read(tag)?;
        let d = p.pointer(24)?;
        if p.array(d + 0x60, 48, Some(0x80802945))
            .is_ok_and(|rows| !rows.is_empty())
        {
            candidates.push((tag, p));
        }
    }
    ensure!(
        candidates.len() == 1,
        "Source firing translator is missing or ambiguous"
    );
    let (tag, payload) = candidates.pop().context("Source firing translator")?;
    let from = Table::read(tag, payload, key, true)?;
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, 0x2F981564).map_err(anyhow::Error::msg)?;
    ensure!(
        bindings.len() == 1,
        "Native firing translator binding differs"
    );
    let binding = &bindings[0];
    let to = Table::read(
        binding.owner_tag,
        (*native.tag(binding.owner_tag, Some(0x80809C36))?).clone(),
        runtime.weapon_translation_group_hash,
        false,
    )?;
    ensure!(
        from.routes.keys().eq(to.routes.keys()),
        "Source and native firing destinations differ"
    );
    let mut channels = BTreeMap::new();
    for (dest, source_route) in &from.routes {
        let target = &to.routes[dest];
        ensure!(
            source_route.logic == target.logic
                && source_route.operands.len() == target.operands.len(),
            "Source and native firing operations differ"
        );
        for (s, n) in source_route.operands.iter().zip(&target.operands) {
            match (s, n) {
                (Operand::Constant(a), Operand::Constant(b)) => {
                    ensure!(a == b, "Firing logic constants differ")
                }
                (Operand::Output(a), Operand::Output(b)) => {
                    ensure!(
                        channels.insert(*b, *a).is_none_or(|old| old == *a),
                        "Conflicting firing output correspondence"
                    );
                }
                _ => anyhow::bail!("Firing operand kinds differ"),
            }
        }
    }
    let mut cells = BTreeMap::new();
    let mut curves = Vec::new();
    for (n, s) in channels {
        let (sr, sc) = from.curve(s)?;
        let (nr, nc) = to.curve(n)?;
        ensure!(
            from.payload.bytes::<24>(sr + 32)? == to.payload.bytes::<24>(nr + 32)?
                && sc.len() == nc.len(),
            "Firing stat selector, interpolation or domain differs"
        );
        let stat = from.payload.u32(sr + 32)?;
        let overrides = |rows: &[[u8; 8]]| {
            rows.iter()
                .filter(|r| u32::from_le_bytes(r[..4].try_into().unwrap()) == stat)
                .copied()
                .collect::<Vec<_>>()
        };
        ensure!(
            overrides(&from.overrides) == overrides(&to.overrides),
            "Firing stat overrides differ"
        );
        let mut values = Vec::new();
        for (source_cell, target_cell) in sc.into_iter().zip(nc) {
            let bytes = from.payload.bytes::<4>(source_cell)?;
            ensure!(
                bytes == to.payload.bytes::<4>(target_cell)?
                    || !to.protected.contains(&target_cell),
                "Firing curve is shared with another input or mode"
            );
            ensure!(
                cells
                    .insert(target_cell, bytes)
                    .is_none_or(|old| old == bytes),
                "Aliased firing curves disagree"
            );
            values.push(from.payload.f32(source_cell)?);
        }
        curves.push(
            json!({"stat_input":stat,"source_channel":s,"native_channel":n,"samples":values}),
        );
    }
    ensure!(!cells.is_empty(), "No firing curves were qualified");
    let edits = cells.into_iter().filter(|(at, bytes)| to.payload.0.get(*at..at + 4) != Some(bytes.as_slice())).map(|(at, bytes)| {
        let offset = u64::try_from(at)?.checked_sub(binding.resource_offset).context("Firing patch precedes its binding")?;
        Ok(json!({"offset":u32::try_from(offset)?,"before":to.payload.bytes::<4>(at)?,"after":bytes}))
    }).collect::<Result<Vec<_>>>()?;
    let report = json!({"status":"translated","source_owner":from.tag,"native_owner":to.tag,
        "native_resource":binding.resource_offset,"native_sha256":hex::encode(Sha256::digest(&to.payload.0)),
        "source_key":from.key,"native_key":to.key,"source_table":from.index,"native_table":to.index,
        "curves":curves,"edits":edits,"scope":"Rate, burst size, interval and charge inputs. Other stat curves and runtime state machines remain native.","gameplay_verified":false});
    Ok(report)
}
