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

pub(super) fn plan(lengths: impl IntoIterator<Item = usize>) -> AuthoringResult<Packing> {
    let mut end = 0usize;
    let mut locations = Vec::new();
    for size in lengths {
        if size == 0 {
            return Err(invalid("A packed resource cannot be empty"));
        }
        let start = end
            .checked_add(15)
            .ok_or_else(|| invalid("Packed alignment overflow"))?
            & !15;
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
    payloads: impl IntoIterator<Item = &'a [u8]>,
    mut emit: impl FnMut(usize, &[u8]) -> AuthoringResult<()>,
) -> AuthoringResult<usize> {
    let mut block = Vec::with_capacity(BLOCK_SIZE);
    let mut count = 0;
    for payload in payloads {
        if payload.is_empty() {
            return Err(invalid("A packed resource cannot be empty"));
        }
        let aligned = (block.len() + 15) & !15;
        block.resize(aligned, 0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_resources_share_blocks_and_cross_boundaries_without_changing_bytes() {
        let payloads = [
            vec![1; 17],
            vec![2; BLOCK_SIZE - 33],
            vec![3; BLOCK_SIZE + 7],
            vec![4; 11],
        ];
        let packing = plan(payloads.iter().map(Vec::len)).unwrap();
        assert_eq!(packing.locations[1].offset, 32);
        assert_eq!(packing.locations[2].block, 1);
        assert_eq!(packing.locations[2].offset, 0);
        assert_eq!(packing.locations[3].offset, 16);
        let mut blocks = Vec::new();
        let count = visit_blocks(payloads.iter().map(Vec::as_slice), |_, bytes| {
            blocks.push(bytes.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(count, packing.block_count);
        let flat = blocks.concat();
        assert_eq!(flat[BLOCK_SIZE - 1], 0);
        for (payload, location) in payloads.iter().zip(packing.locations) {
            let start = location.block * BLOCK_SIZE + location.offset;
            assert_eq!(&flat[start..start + payload.len()], payload);
        }
    }

    #[test]
    fn tiny_resources_do_not_consume_one_block_each() {
        let packing = plan(std::iter::repeat_n(8, 6000)).unwrap();
        assert_eq!(packing.locations.len(), 6000);
        assert_eq!(packing.block_count, 1);
        assert!(plan([0]).is_err());
        assert!(plan([usize::MAX, 16]).is_err());
    }

    #[test]
    fn packed_block_writer_errors_stop_emission() {
        let bytes = vec![0; BLOCK_SIZE * 2];
        let mut calls = 0;
        assert!(
            visit_blocks([bytes.as_slice()], |_, _| {
                calls += 1;
                Err(invalid("write failed"))
            })
            .is_err()
        );
        assert_eq!(calls, 1);
    }
}
