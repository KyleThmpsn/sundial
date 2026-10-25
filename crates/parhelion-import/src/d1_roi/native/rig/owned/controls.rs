//! Source defaults and group identities on matching native control operations.
use super::*;

pub(super) fn convert(
    sc: &Payload,
    original: &Payload,
    changed_bones: &BTreeSet<usize>,
) -> Result<(Payload, BTreeSet<usize>)> {
    let mut nc = original.clone();
    let (sr, nr) = (sc.pointer(24)?, nc.pointer(24)?);
    let sg = groups(sc, sr + 0x90, 0x80800380)?;
    let ng = groups(&nc, nr + 0x90, 0x8080907F)?;
    ensure!(
        sg.len() == ng.len()
            && sg.len() >= 2
            && sg[..sg.len() - 1] == ng[..ng.len() - 1]
            && sg.last().unwrap().0 != ng.last().unwrap().0
            && sg.last().unwrap().1 == ng.last().unwrap().1,
        "Private weapon needs an independently named trailing control group"
    );
    let common = sg[..sg.len() - 1]
        .iter()
        .map(|g| g.1 as usize)
        .sum::<usize>();
    let so = sc.array(sr + 0xA0, 48, Some(0x80800638))?;
    let no = nc.array(nr + 0xA0, 52, Some(0x80809076))?;
    ensure!(so.len() == no.len(), "Private control slot count differs");
    for (&a, &b) in so.iter().zip(&no) {
        ensure!(
            sc.0[a..a + 48] == nc.0[b + 4..b + 52],
            "Private control operations differ"
        );
    }
    let forward = sc.array(sr + 0xD8, 2, Some(0x80800006))?;
    let mut changed_slots = BTreeSet::new();
    for &bone in changed_bones {
        let slot = usize::from(sc.u16(*forward.get(bone).context("Private bone slot")?)?);
        ensure!(
            (common..no.len()).contains(&slot),
            "Changed bone escapes the private control group"
        );
        changed_slots.insert(slot);
    }
    for (field, stride, sclass, nclass) in [
        (0x90, 8, 0x80800380, 0x8080907F),
        (0xB0, 32, 0x808001B0, 0x80809F75),
        (0xC0, 2, 0x80800006, 0x80800006),
        (0xD8, 2, 0x80800006, 0x80800006),
        (0xE8, 2, 0x80800006, 0x80800006),
    ] {
        let a = sc.array_range(sr + field, stride, Some(sclass))?;
        let b = nc.array_range(nr + field, stride, Some(nclass))?;
        ensure!(
            a.len() == b.len(),
            "Private control array dimensions differ"
        );
        if field == 0xB0 {
            for (slot, (s, n)) in sc.0[a.clone()]
                .chunks_exact(32)
                .zip(nc.0[b.clone()].chunks_exact(32))
                .enumerate()
            {
                if s != n {
                    ensure!(slot >= common, "Private default changes an arm control");
                    changed_slots.insert(slot);
                }
            }
        } else if field != 0x90 {
            ensure!(
                sc.0[a.clone()] == nc.0[b.clone()],
                "Private control lookup changes"
            );
        }
        nc.0[b].copy_from_slice(&sc.0[a]);
    }
    let names = nc
        .array(nr + 0x110, 8, Some(0x80808FFE))?
        .into_iter()
        .map(|a| Ok((nc.u32(a)?, nc.u32(a + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for a in sc.array(sr + 0x110, 8, Some(0x808006C8))? {
        ensure!(
            names.get(&sc.u32(a)?) == Some(&sc.u32(a + 4)?),
            "Source named control has no native binding"
        );
    }
    for &bone in names.values() {
        ensure!(
            !changed_bones.contains(&(bone as usize)),
            "A native named controller targets a changed weapon bone"
        );
    }
    let sstate = sc.array_range(sc.pointer(16)? + 0x30, 48, Some(0x80800502))?;
    let nstate = nc.array_range(nc.pointer(16)? + 0x30, 48, Some(0x80808F92))?;
    ensure!(
        sc.0[sstate] == nc.0[nstate],
        "Private control instance state differs"
    );
    crate::tiger::animation::rig::controls::validate(&nc.0)?;
    Ok((nc, changed_slots))
}
