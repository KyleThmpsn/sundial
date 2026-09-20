//! Preserve numeric programs when native stat-contribution rows move.
use super::*;

type StatProgramTargets = BTreeMap<u16, [Option<usize>; 2]>;
const PROGRAM_OFFSETS: [usize; 2] = [8, 24];

pub(in crate::weapon) fn stat_program_targets(
    data: &[u8],
    rows: usize,
    count: usize,
) -> AuthoringResult<StatProgramTargets> {
    let mut targets = BTreeMap::new();
    for index in 0..count {
        let row = rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
        let mut programs = [None; 2];
        for (lane, offset) in PROGRAM_OFFSETS.into_iter().enumerate() {
            let descriptor = row + offset;
            validate_item_numeric_program(data, descriptor).map_err(|error| {
                error.context(format!("Investment stat row {index}, program {lane}"))
            })?;
            if read_i64(data, descriptor + 8)? != 0 {
                programs[lane] = Some(relative_target(data, descriptor + 8)?);
            }
        }
        targets.insert(u16::from(read_u8(data, row)?), programs);
    }
    Ok(targets)
}

pub(in crate::weapon) fn relocate_stat_programs(
    data: &mut [u8],
    rows: usize,
    count: usize,
    targets: &StatProgramTargets,
) -> AuthoringResult<()> {
    for index in 0..count {
        let row = rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
        let definition = u16::from(read_u8(data, row)?);
        if let Some(programs) = targets.get(&definition) {
            for (lane, target) in programs.iter().enumerate() {
                if let Some(target) = target {
                    write_relative_pointer(data, row + PROGRAM_OFFSETS[lane] + 8, *target)?;
                }
            }
        }
    }
    stat_program_targets(data, rows, count)?;
    Ok(())
}
