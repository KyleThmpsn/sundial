//! The twelve-vector coverage scope writes either clipped or premultiplied deferred targets.
use super::*;

pub(super) fn supported(code: &Code) -> bool {
    if code.buffers != [(4, 12)]
        || code.resources.len() != 3
        || !(3..=5).all(|slot| {
            code.resources
                .iter()
                .any(|r| r.slot == slot && r.dimension == 3 && !r.integer)
        })
    {
        return false;
    }
    let writers = code
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            row.operands
                .first()
                .is_some_and(|o| o.kind == 2 && o.indices[0].base == 1 && o.mask & 7 != 0)
        })
        .collect::<Vec<_>>();
    // Both paths must preserve the native normal-radius representation. In the blend path,
    // the packed normal is multiplied by coverage before it reaches the target.
    writers.len() == 2
        && writers.into_iter().all(|(at, row)| {
            normal::conditional(code, at, row)
                || row.code == 56
                    && !row.saturate
                    && row.operands[0].mask == 7
                    && (1..=2).any(|source| {
                        producer(code, &row.operands[source], 0, at).is_some_and(
                            |(before, packed, _)| {
                                (1..3).all(|lane| {
                                    producer(code, &row.operands[source], lane, at)
                                        .is_some_and(|(other, _, _)| other == before)
                                }) && normal::conditional(code, before, packed)
                            },
                        )
                    })
        })
}
