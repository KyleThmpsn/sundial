//! Retained allocation trees and replica rows are validated before the cue obligation is published.
use super::*;

fn row(p: &Payload, at: usize, name: u32, class: u32, count: u32) -> Result<Vec<usize>> {
    ensure!(
        p.u64(at)? == name as u64
            && p.u64(at + 8)? == 0
            && p.u32(at + 16)? == class
            && p.u32(at + 20)? == count,
        "Native audio allocation member differs"
    );
    p.array(at + 24, 40, Some(0x80808852))
}

fn sequence(p: &Payload) -> Result<()> {
    let roots = p.array(32, 40, Some(0x80808852))?;
    ensure!(
        roots.len() == 5,
        "Native audio allocation has additional roots"
    );
    for (index, name, class) in [(0, 0xC78E66C7, 0x808093D6), (1, 0xE3CC5C98, 0x80806B37)] {
        let branches = row(p, roots[index], name, 0x808093E5, 1)?;
        ensure!(
            branches.len() == 1,
            "Native audio allocation element count differs"
        );
        let leaves = row(p, branches[0], 0x811C9DC5, u32::MAX, 0)?;
        ensure!(
            leaves.len() == 1 && row(p, leaves[0], 0x3988F7D4, class, 1)?.is_empty(),
            "Native audio node allocation differs"
        );
    }
    for (index, name) in [(2, 0xE3EA46ED), (3, 0x5C5BD59A)] {
        ensure!(
            row(p, roots[index], name, 0x8080000A, 1)?.is_empty(),
            "Native audio selector allocation differs"
        );
    }
    let locators = row(p, roots[4], 0x846FDE3F, u32::MAX, 0)?;
    ensure!(
        locators.len() == 3,
        "Native audio locator allocation count differs"
    );
    for (at, (name, class)) in locators.into_iter().zip([
        (0x4C2E87A1, 0x80809F75),
        (0x6E385590, 0x80808618),
        (0xA6E3DD20, 0x80809BFF),
    ]) {
        ensure!(
            row(p, at, name, class, 1)?.is_empty(),
            "Native audio locator allocation differs"
        );
    }
    Ok(())
}

pub(super) fn read(native: &mut Reader, graph: &Payload, owners: &[u32]) -> Result<()> {
    let replica = native.tag(graph.u32(0x88)?, Some(0x80809BB6))?;
    let rows = replica.array(48, 32, Some(0x80809BB8))?;
    ensure!(
        replica.u64(0)? == replica.0.len() as u64
            && replica.u32(8)? == GRAPH
            && replica.0.get(12..48) == Some(&[0; 36][..])
            && rows.len() == owners.len(),
        "Native audio replica does not match its host"
    );
    for (index, (&owner, at)) in owners.iter().zip(rows).enumerate() {
        let p = native.tag(owner, Some(0x80809C36))?;
        let allocation = native.tag(p.u32(0x44)?, Some(0x80809BBB))?;
        ensure!(
            replica.u32(at)? == p.u32(p.pointer(16)? - 4)?
                && replica.u32(at + 4)? == p.u32(0x44)?
                && replica.bytes::<16>(at + 8)? == [0; 16]
                && replica.u32(at + 28)? == 0
                && allocation.u64(0)? == allocation.0.len() as u64
                && allocation.u64(8)? == 0x811C9DC5
                && allocation.u64(16)? == 0
                && allocation.u64(24)? == u32::MAX as u64,
            "Native audio allocation or replica row differs"
        );
        // The complete stock row, including +18's opaque word, stays intact during
        // private publication. No allocation word is derived from this validation.
        if index == 1 {
            sequence(&allocation)?;
        } else {
            ensure!(
                allocation.0.len() == 48 && allocation.bytes::<16>(32)? == [0; 16],
                "Native status or network allocation is not empty"
            );
        }
    }
    Ok(())
}
