//! Rise of Iron stat budgets: a zero-valued talent stat row on a budgeted stat takes its slot's
//! share of the level budget, which an armor host carries as Shadowkeep points tier for tier.
//!
//! Source row reader `1045230` sums an item's talent rows by stat ordinal at `+0C`. Selector `FF`
//! at `+0D` takes the fixed fraction at `+04`. Any other selector picks one of the item instance's
//! random roll bytes and interpolates `[+04, +08]`, which is the item's own stat roll. Contribution
//! `102AD00` returns `trunc(value * fraction)` for a nonzero value. A zero value instead returns
//! `trunc(share * budget * fraction)`, where the budget comes from the stat's `8080173D` object in
//! table `80A5E27E` (class `808017EC`) and the share is the slot's weight over the summed weights
//! when the stat's kind at `+04` is nonzero. Intellect, Discipline and Strength share one curve,
//! flat at 516 from the coordinate of Light 340, so every Rise of Iron endgame item sits on it, and
//! weights helmet 9, gauntlets 8, chest 12, legs 11 and class item 5 of 60. Their tier reader
//! normalizes points by 300 into tiers 0 to 5, 60 points a tier, where Shadowkeep has 10.
use super::*;

/// Source points per tier for the budgeted character stats, and Shadowkeep's.
const SOURCE_POINTS_PER_TIER: i64 = 60;
const NATIVE_POINTS_PER_TIER: i64 = 10;

/// One budgeted stat: its share kind, its saturated budget and its slot weights.
pub(super) struct Budget {
    kind: u8,
    peak: i32,
    weights: Vec<(u8, i16)>,
}

/// Every budgeted source stat by hash.
pub(super) fn budgets(source: &Source) -> Result<BTreeMap<u32, Budget>> {
    let tags = source.classes(0x808017EC);
    ensure!(tags.len() == 1, "Source stat table is missing or ambiguous");
    let table = source.read(tags[0])?;
    let mut budgets = BTreeMap::new();
    for row in table.array(8, 16, Some(0x80801870))? {
        if table.u64(row + 8)? == 0 {
            continue;
        }
        let object = table.pointer(row + 8)?;
        ensure!(
            table.u32(object - 4)? == 0x8080173D,
            "Source stat budget object class differs"
        );
        let levels = table
            .array(object, 8, None)?
            .into_iter()
            .map(|at| Ok((table.u32(at)? as i32, table.u32(at + 4)? as i32)))
            .collect::<Result<Vec<_>>>()?;
        let weights = table
            .array(object + 0x20, 4, None)?
            .into_iter()
            .map(|at| Ok((table.u8(at + 1)?, table.u16(at + 2)? as i16)))
            .collect::<Result<Vec<_>>>()?;
        let peak = levels
            .iter()
            .map(|&(_, y)| y)
            .max()
            .context("Source stat budget is empty")?;
        // The maximum must be the curve's saturated end, the value endgame items take.
        ensure!(
            levels.last().map(|&(_, y)| y) == Some(peak),
            "Source stat budget does not saturate"
        );
        budgets.insert(
            table.u32(row)?,
            Budget {
                kind: table.u8(row + 4)?,
                peak,
                weights,
            },
        );
    }
    Ok(budgets)
}

/// A fixed-fraction zero-valued row's Shadowkeep points for an armor `slot`, tier for tier.
pub(super) fn armor_points(budget: &Budget, slot: u8, fraction: f32) -> Result<i32> {
    let total = budget
        .weights
        .iter()
        .map(|&(_, weight)| i32::from(weight))
        .sum::<i32>();
    let weight = budget
        .weights
        .iter()
        .find(|&&(candidate, _)| candidate == slot)
        .map(|&(_, weight)| i32::from(weight))
        .with_context(|| format!("Source stat budget has no share for armor slot {slot}"))?;
    ensure!(
        budget.kind != 0 && total > 0 && weight > 0 && fraction.is_finite() && fraction >= 0.0,
        "Source stat budget share is undefined"
    );
    // The same Float32 order as 102AD00, truncated toward zero.
    let share = weight as f32 / total as f32;
    let source = (share * budget.peak as f32 * fraction) as i64;
    let scaled = source * NATIVE_POINTS_PER_TIER;
    Ok(i32::try_from(
        (scaled + SOURCE_POINTS_PER_TIER / 2).div_euclid(SOURCE_POINTS_PER_TIER),
    )?)
}
