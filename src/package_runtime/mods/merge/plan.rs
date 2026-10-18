//! Row shifts and site checks (Sunrise f1e7c66 `table_merge/plan.cpp`). Rows each mod adds to a
//! shared table follow the rows earlier mods added, so the fields that name them move too.
use super::sidecar::{
    Entry, OP_LOCAL_MASK, OP_LOCAL_TARGET, Op, OpKind, SIGNED_SITE, Sidecar, Site, TOP_LEVEL_OP,
};
use super::{ENTRY_BITS, EntryInputs, Input, Status, TAG_BASE, entry_of, tag_of};
use std::collections::{BTreeMap, BTreeSet};

/// One index space: a table tag and the descriptor of its array.
type TargetKey = (u32, u32);

/// Rows to add to each mod's site values, per target. Absent means 0.
#[derive(Default)]
pub(super) struct Shifts {
    rows: BTreeMap<(usize, u32, u32), u32>,
}

impl Shifts {
    fn set(&mut self, owner: usize, table: u32, descriptor: u32, rows: u32) {
        self.rows.insert((owner, table, descriptor), rows);
    }
    pub(super) fn get(&self, owner: usize, table: u32, descriptor: u32) -> u32 {
        self.rows
            .get(&(owner, table, descriptor))
            .copied()
            .unwrap_or(0)
    }
    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }
    /// Every planned shift: the mod, the table, the array's descriptor and the rows.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, u32, u32, u32)> + '_ {
        self.rows
            .iter()
            .map(|(&(owner, table, descriptor), &rows)| (owner, table, descriptor, rows))
    }
}

/// The top-level append op at `descriptor` one input declares.
fn top_op<'a>(input: &Input<'a>, entry: u32, descriptor: u32) -> Option<&'a Op> {
    entry_of(input, entry)?.ops.iter().find(|op| {
        op.parent_op == TOP_LEVEL_OP && op.kind == OpKind::Append && op.descriptor == descriptor
    })
}

/// The smallest and largest value one site can hold.
fn site_range(site: &Site) -> (i64, i64) {
    let bits = u32::from(site.width) * 8;
    if site.flags & SIGNED_SITE != 0 {
        let half = 1i64 << (bits - 1);
        (-half, half - 1)
    } else {
        (0, (1i64 << bits) - 1)
    }
}

/// The site's value at its width and sign. The caller has checked the bounds.
fn read_site(bytes: &[u8], site: &Site) -> i64 {
    let at = site.offset as usize;
    let mut raw = [0u8; 4];
    raw[..usize::from(site.width)].copy_from_slice(&bytes[at..at + usize::from(site.width)]);
    let raw = u32::from_le_bytes(raw);
    let bits = u32::from(site.width) * 8;
    // Sign-extend from the site's own width.
    let negative = site.flags & SIGNED_SITE != 0 && (raw >> (bits - 1)) & 1 != 0;
    let value = i64::from(raw);
    if negative {
        value - (1i64 << bits)
    } else {
        value
    }
}

/// The `[stock count, mod count)` range a site must name a row in.
fn row_range(sidecar: &Sidecar, entry: &Entry, site: &Site) -> (u32, u32) {
    if site.target & OP_LOCAL_TARGET != 0 {
        let op = &entry.ops[usize::from(site.target & OP_LOCAL_MASK)];
        return (op.stock_count, op.mod_count);
    }
    let target = &sidecar.targets[usize::from(site.target)];
    (target.stock_count, target.mod_count)
}

/// Plans the row shifts across every entry in a merge.
struct Planner<'a, 'b> {
    entries: &'b [EntryInputs<'a>],
    shifts: &'b mut Shifts,
    by_entry: BTreeMap<(u16, u32), &'b EntryInputs<'a>>,
    stock_counts: BTreeMap<TargetKey, u32>,
    /// Each mod's final count per target, keyed (table, descriptor, mod).
    records: BTreeMap<(u32, u32, usize), u32>,
}

/// Whether `(owner, sidecar)` was seen, noting it if not. One sidecar serves every entry of its
/// package, so its targets and pair groups are read once.
fn first_sight(seen: &mut BTreeSet<(usize, usize)>, owner: usize, sidecar: &Sidecar) -> bool {
    seen.insert((owner, std::ptr::from_ref(sidecar) as usize))
}

impl<'a, 'b> Planner<'a, 'b> {
    fn run(&mut self) -> Result<(), (Status, u32)> {
        self.collect()?;
        let stock_counts = self.stock_counts.clone();
        for (target, stock_count) in stock_counts {
            self.shift(target, stock_count)
                .map_err(|status| (status, target.0))?;
        }
        self.check_pairs()
    }

    /// Indexes the entries and every mod's target records. One target keeps one stock count.
    fn collect(&mut self) -> Result<(), (Status, u32)> {
        let mut seen = BTreeSet::new();
        for entry in self.entries {
            self.by_entry
                .entry((entry.package, entry.entry))
                .or_insert(entry);
            for input in &entry.inputs {
                let Some(sidecar) = input.sidecar else {
                    continue;
                };
                if !first_sight(&mut seen, input.owner, sidecar) {
                    continue;
                }
                for target in &sidecar.targets {
                    let key = (target.table, target.descriptor);
                    let stock = *self.stock_counts.entry(key).or_insert(target.stock_count);
                    let record = *self
                        .records
                        .entry((key.0, key.1, input.owner))
                        .or_insert(target.mod_count);
                    if stock != target.stock_count || record != target.mod_count {
                        return Err((Status::Mismatch, key.0));
                    }
                }
            }
        }
        Ok(())
    }

    /// Gives every contributor to one target the rows that earlier contributors added.
    fn shift(&mut self, target: TargetKey, stock_count: u32) -> Result<(), Status> {
        let (tag, descriptor) = target;
        if tag < TAG_BASE {
            return Err(Status::Mismatch);
        }
        let package =
            u16::try_from((tag - TAG_BASE) >> ENTRY_BITS).map_err(|_| Status::Mismatch)?;
        let records = self
            .records
            .range((tag, descriptor, 0)..=(tag, descriptor, usize::MAX))
            .map(|(&(_, _, owner), &count)| (owner, count))
            .collect::<Vec<_>>();
        // An entry outside the merge has one contributor at most, so nothing moves. Its count
        // changes in one mod's records at most.
        let entry = tag & ((1 << ENTRY_BITS) - 1);
        let Some(found) = self.by_entry.get(&(package, entry)).copied() else {
            let changed = records
                .iter()
                .filter(|&&(_, count)| count != stock_count)
                .count();
            return if changed > 1 {
                Err(Status::Mismatch)
            } else {
                Ok(())
            };
        };
        let mut added = 0u64;
        let mut declared: Option<&Op> = None;
        let mut carried = BTreeMap::new();
        for input in &found.inputs {
            if added != 0 {
                self.shifts.set(input.owner, tag, descriptor, added as u32);
            }
            let Some(op) = top_op(input, found.entry, descriptor) else {
                continue;
            };
            // Every declaring mod must give the same limit.
            let first = *declared.get_or_insert(op);
            if op.stock_count != stock_count || op.limit != first.limit {
                return Err(Status::Mismatch);
            }
            added += u64::from(op.mod_count - op.stock_count);
            carried.insert(input.owner, op.mod_count);
        }
        if declared.is_some_and(|op| u64::from(stock_count) + added > u64::from(op.limit)) {
            return Err(Status::Limit);
        }
        // Each mod's own count must match the op it ships, or the stock count when it ships none.
        for (owner, count) in records {
            if count != carried.get(&owner).copied().unwrap_or(stock_count) {
                return Err(Status::Mismatch);
            }
        }
        Ok(())
    }

    /// Checks that each mod adds the same row count to every member of each pair group. A member a
    /// mod doesn't change counts as 0.
    fn check_pairs(&self) -> Result<(), (Status, u32)> {
        let mut members = BTreeMap::<u32, BTreeSet<TargetKey>>::new();
        let mut added = BTreeMap::<(u32, usize), BTreeMap<TargetKey, u32>>::new();
        let mut seen = BTreeSet::new();
        // Every entry of each sidecar counts, so a member outside the merge is still checked.
        for entry in self.entries {
            for input in &entry.inputs {
                let Some(sidecar) = input.sidecar else {
                    continue;
                };
                if !first_sight(&mut seen, input.owner, sidecar) {
                    continue;
                }
                for spec in &sidecar.entries {
                    let tag = tag_of(sidecar.package, spec.index);
                    for op in &spec.ops {
                        if op.parent_op != TOP_LEVEL_OP
                            || op.kind != OpKind::Append
                            || op.pair_group == 0
                        {
                            continue;
                        }
                        let key = (tag, op.descriptor);
                        members.entry(op.pair_group).or_default().insert(key);
                        added
                            .entry((op.pair_group, input.owner))
                            .or_default()
                            .insert(key, op.mod_count - op.stock_count);
                    }
                }
            }
        }
        for ((group, _), counts) in &added {
            let group = &members[group];
            let first = group.first().expect("a pair group has a member");
            let expected = counts.get(first).copied().unwrap_or(0);
            for member in group {
                if counts.get(member).copied().unwrap_or(0) != expected {
                    return Err((Status::Pairs, member.0));
                }
            }
        }
        Ok(())
    }
}

/// Plans every mod's shift for every target from the sidecars alone. The error names the table
/// that turned the row layer off.
pub(super) fn plan_shifts(
    entries: &[EntryInputs<'_>],
    shifts: &mut Shifts,
) -> Result<(), (Status, u32)> {
    shifts.clear();
    Planner {
        entries,
        shifts,
        by_entry: BTreeMap::new(),
        stock_counts: BTreeMap::new(),
        records: BTreeMap::new(),
    }
    .run()
}

/// Checks that every site value names a row its mod added.
pub(super) fn check_sites(input: &Input<'_>, entry: u32) -> Result<(), Status> {
    let (Some(sidecar), Some(spec)) = (input.sidecar, entry_of(input, entry)) else {
        return Ok(());
    };
    for site in &spec.sites {
        // Only append rows keep their numbers. Sorted map rows move by key.
        let local = site.target & OP_LOCAL_TARGET != 0;
        if site.offset as usize + usize::from(site.width) > input.bytes.len()
            || (local && spec.ops[usize::from(site.target & OP_LOCAL_MASK)].kind != OpKind::Append)
        {
            return Err(Status::Sites);
        }
        let (first, end) = row_range(sidecar, spec, site);
        let value = read_site(input.bytes, site);
        if value < i64::from(first) || value >= i64::from(end) {
            return Err(Status::Sites);
        }
    }
    Ok(())
}

/// Adds each site's planned shift to `copy` in place.
pub(super) fn shift_sites(
    input: &Input<'_>,
    entry: u32,
    shifts: &Shifts,
    op_local_shifts: &[u32],
    copy: &mut [u8],
) -> Result<(), Status> {
    let (Some(sidecar), Some(spec)) = (input.sidecar, entry_of(input, entry)) else {
        return Ok(());
    };
    for site in &spec.sites {
        let rows = if site.target & OP_LOCAL_TARGET != 0 {
            op_local_shifts
                .get(usize::from(site.target & OP_LOCAL_MASK))
                .copied()
                .unwrap_or(0)
        } else {
            let target = &sidecar.targets[usize::from(site.target)];
            shifts.get(input.owner, target.table, target.descriptor)
        };
        if rows == 0 {
            continue;
        }
        let at = site.offset as usize;
        if at + usize::from(site.width) > copy.len() {
            return Err(Status::Format);
        }
        let value = read_site(copy, site) + i64::from(rows);
        if value > site_range(site).1 {
            return Err(Status::Limit);
        }
        // Little-endian two's complement: the low `width` bytes are the value.
        let raw = (value as u32).to_le_bytes();
        copy[at..at + usize::from(site.width)].copy_from_slice(&raw[..usize::from(site.width)]);
    }
    Ok(())
}
