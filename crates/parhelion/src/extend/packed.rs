//! Native entry offsets let standalone resources share physical payload blocks.
use super::*;

pub(super) struct Location {
    pub block: usize,
    pub offset: usize,
}

pub(super) struct Packing {
    pub locations: Vec<Location>,
    pub block_count: usize,
}

pub(super) fn plan(entries: impl IntoIterator<Item = (usize, usize)>) -> AuthoringResult<Packing> {
    let mut end = 0usize;
    let mut locations = Vec::new();
    for (size, alignment) in entries {
        if size == 0 {
            return Err(invalid("A packed resource cannot be empty"));
        }
        let start = aligned(end, alignment)?;
        locations.push(Location {
            block: start / BLOCK_SIZE,
            offset: start % BLOCK_SIZE,
        });
        end = start
            .checked_add(size)
            .ok_or_else(|| invalid("Packed payload size overflow"))?;
    }
    Ok(Packing {
        locations,
        block_count: end.div_ceil(BLOCK_SIZE),
    })
}

pub(super) fn visit_blocks<'a>(
    payloads: impl IntoIterator<Item = (&'a [u8], usize)>,
    mut emit: impl FnMut(usize, &[u8]) -> AuthoringResult<()>,
) -> AuthoringResult<usize> {
    let mut block = Vec::with_capacity(BLOCK_SIZE);
    let mut count = 0;
    for (payload, alignment) in payloads {
        if payload.is_empty() {
            return Err(invalid("A packed resource cannot be empty"));
        }
        block.resize(aligned(block.len(), alignment)?, 0);
        let mut remaining = payload;
        while !remaining.is_empty() {
            let taken = remaining.len().min(BLOCK_SIZE - block.len());
            block.extend_from_slice(&remaining[..taken]);
            remaining = &remaining[taken..];
            if block.len() == BLOCK_SIZE {
                emit(count, &block)?;
                count += 1;
                block.clear();
            }
        }
    }
    if !block.is_empty() {
        emit(count, &block)?;
        count += 1;
    }
    Ok(count)
}

fn aligned(offset: usize, alignment: usize) -> AuthoringResult<usize> {
    if !alignment.is_power_of_two() || !(16..=BLOCK_SIZE).contains(&alignment) {
        return Err(invalid("Packed resource alignment is unsupported"));
    }
    offset
        .checked_add(alignment - 1)
        .map(|end| end & !(alignment - 1))
        .ok_or_else(|| invalid("Packed alignment overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_resources_do_not_consume_one_block_each() {
        let packing = plan(std::iter::repeat_n((8, 16), 6000)).unwrap();
        assert_eq!(packing.locations.len(), 6000);
        assert_eq!(packing.block_count, 1);
        assert!(plan([(0, 16)]).is_err());
        assert!(plan([(usize::MAX, 16), (16, 16)]).is_err());
    }

    #[test]
    fn packed_block_writer_errors_stop_emission() {
        let bytes = vec![0; BLOCK_SIZE * 2];
        let mut calls = 0;
        assert!(
            visit_blocks([(bytes.as_slice(), 16)], |_, _| {
                calls += 1;
                Err(invalid("write failed"))
            })
            .is_err()
        );
        assert_eq!(calls, 1);
    }
}
