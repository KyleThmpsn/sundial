//! Keep a retained native clip's original shared-control prefix when a private group would widen it.
use super::*;

pub(super) fn retain(
    clip: &Payload,
    original: &[(u32, u32)],
    private: &[(u32, u32)],
    changed: &[usize],
) -> Result<(Option<Payload>, usize)> {
    let declared = groups(clip, 0x170, 0x8080907F)?;
    let common = |a: &[(u32, u32)], b: &[(u32, u32)]| {
        a.iter()
            .zip(b)
            .take_while(|(x, y)| x == y)
            .map(|(x, _)| x.1 as usize)
            .sum::<usize>()
    };
    let current = common(&declared, private);
    if changed.iter().all(|&slot| slot >= current) {
        return Ok((None, current));
    }
    ensure!(
        declared == private
            && original.len() == private.len()
            && private.len() >= 2
            && original[..original.len() - 1] == private[..private.len() - 1]
            && original.last().unwrap().0 != private.last().unwrap().0
            && original.last().unwrap().1 == private.last().unwrap().1,
        "Retained native clip cannot preserve its original control prefix"
    );
    let before = common(&declared, original);
    ensure!(
        changed.iter().all(|&slot| slot >= before),
        "Retained native clip originally writes a changed weapon control"
    );
    let mut output = clip.clone();
    let rows = output.array(0x170, 8, Some(0x8080907F))?;
    let last = *rows.last().context("Retained clip group")?;
    put(
        &mut output.0,
        last,
        &original.last().unwrap().0.to_le_bytes(),
    )?;
    ensure!(
        common(&groups(&output, 0x170, 0x8080907F)?, private) == before,
        "Retained clip changes its native control coverage"
    );
    Ok((Some(output), before))
}
