use super::*;
use crate::catalog::{RecordProgress, RecordRuntime};

fn array(data: &[u8], at: usize, class: u32, stride: usize) -> Result<(usize, usize), String> {
    if u64_at(data, at)? == 0 {
        return Ok((0, 0));
    }
    let (count, rows, actual) = array_at(data, at)?;
    if actual != class
        || count
            .checked_mul(stride)
            .and_then(|size| rows.checked_add(size))
            .is_none_or(|end| end > data.len())
    {
        return Err("The record's inline table has an invalid class or extent".into());
    }
    Ok((count, rows))
}

// Sunrise package_record_build.cpp assigns a sequential account-value run, starting at 2746.
// An empty objective list still reserves two slots. Interval thresholds use that reserved run.
pub(super) fn read(
    data: &[u8],
    at: usize,
    display: &[u8],
    display_at: usize,
    objectives: &[ObjectiveDef],
    next_slot: &mut usize,
) -> Result<RecordRuntime, String> {
    let (count, rows) = array(data, at + 48, 0x80807455, 2)?;
    let (interval_count, intervals) = array(data, at + 64, 0x80802C0F, 12)?;
    let (reward_count, rewards) = array(display, display_at + 72, 0x80805A9B, 24)?;
    let mut runtime = RecordRuntime {
        score: u16::try_from(u32_at(data, at + 92)?).unwrap_or(0),
        ..Default::default()
    };
    let mut add_progress = |objective: usize, lane: usize| -> Result<(), String> {
        let definition = objectives
            .get(objective)
            .ok_or("The record names an unknown objective")?;
        runtime.progress.push(RecordProgress {
            objective,
            slot: u16::try_from(*next_slot + lane)
                .map_err(|_| "Record progress exceeds the account bank")?,
            threshold: definition.completion_value,
        });
        Ok(())
    };
    for lane in 0..count {
        add_progress(usize::from(u16_at(data, rows + lane * 2)?), lane)?;
    }
    if count == 0 && interval_count != 0 {
        let objective = u32_at(data, intervals + (interval_count - 1) * 12)? as usize;
        for lane in 0..2 {
            add_progress(objective, lane)?;
        }
    }
    for row in 0..interval_count {
        runtime
            .interval_scores
            .push(u32_at(data, intervals + row * 12 + 4)?);
        let item = u16_at(data, intervals + row * 12 + 8)?;
        runtime
            .interval_items
            .push((item != u16::MAX).then_some(usize::from(item)));
    }
    for row in 0..reward_count {
        let item = u32_at(display, rewards + row * 24)?;
        let quantity = u32_at(display, rewards + row * 24 + 4)?;
        if item < u32::from(u16::MAX) && quantity > 0 && quantity <= i32::MAX as u32 {
            runtime.rewards.push((item as usize, quantity as i32));
        }
    }
    *next_slot = next_slot
        .checked_add(count.max(2 * usize::from(count == 0)))
        .ok_or("Record progress bank overflowed")?;
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inline(data: &mut [u8], at: usize, header: usize, count: u64, class: u32) {
        data[at..at + 8].copy_from_slice(&count.to_le_bytes());
        data[at + 8..at + 16].copy_from_slice(&((header - at - 8) as i64).to_le_bytes());
        data[header..header + 8].copy_from_slice(&count.to_le_bytes());
        data[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
    }
    #[test]
    fn record_runs_reserve_empty_slots_and_decode_interval_rewards_and_score() {
        let mut data = vec![0; 300];
        let display = vec![0; 128];
        let mut slot = 2746;
        read(&data, 0, &display, 0, &[], &mut slot).unwrap();
        assert_eq!(slot, 2748);
        inline(&mut data, 64, 224, 2, 0x80802C0F);
        data[244..248].copy_from_slice(&10_u32.to_le_bytes());
        data[248..250].copy_from_slice(&u16::MAX.to_le_bytes());
        data[256..260].copy_from_slice(&20_u32.to_le_bytes());
        data[260..262].copy_from_slice(&7_u16.to_le_bytes());
        data[92..96].copy_from_slice(&25_u32.to_le_bytes());
        let runtime = read(
            &data,
            0,
            &display,
            0,
            &[ObjectiveDef {
                completion_value: 8,
                ..Default::default()
            }],
            &mut slot,
        )
        .unwrap();
        assert_eq!(slot, 2750);
        assert_eq!(
            runtime.progress.iter().map(|p| p.slot).collect::<Vec<_>>(),
            vec![2748, 2749]
        );
        assert_eq!(runtime.score, 25);
        assert_eq!(runtime.interval_scores, vec![10, 20]);
        assert_eq!(runtime.interval_items, vec![None, Some(7)]);
        data[232..236].copy_from_slice(&99_u32.to_le_bytes());
        assert!(read(&data, 0, &display, 0, &[], &mut slot).is_err());
    }
}
