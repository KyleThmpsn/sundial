//! Source bone tables within an unchanged native FK and procedural envelope.
use super::*;

pub(super) fn convert(
    ss: &Payload,
    original: &Payload,
    sb: &[Bone],
    nb: &[Bone],
    weapon_start: usize,
) -> Result<(Payload, BTreeSet<usize>)> {
    let mut ns = original.clone();
    let (sr, nr) = (ss.pointer(24)?, ns.pointer(24)?);
    let (si, ni) = (ss.pointer(16)?, ns.pointer(16)?);
    ensure!(
        sb.len() < 256 && sb[weapon_start] == nb[weapon_start],
        "Private rig changes the weapon root"
    );
    let mut changed_bones = BTreeSet::new();
    for (i, (a, b)) in sb.iter().zip(nb).enumerate() {
        ensure!(
            a.0 == i && b.0 == i && a.2 == b.2 && a.2 >= -1 && a.2 < i as i32,
            "Private rig hierarchy topology differs"
        );
        if a != b {
            ensure!(i > weapon_start, "Private rig changes a shared bone");
            changed_bones.insert(i);
        }
    }
    ensure!(
        !changed_bones.is_empty(),
        "Private rig has no changed weapon section"
    );
    ensure!(
        sb.iter().map(|b| &b.1).collect::<BTreeSet<_>>().len() == sb.len(),
        "Private rig repeats bone names"
    );
    for (i, stride) in [16, 32, 32, 2, 2].into_iter().enumerate() {
        let sc = [0x808004F4, 0x808001B0, 0x808001B0, 0x80800006, 0x80800006][i];
        let nc = [0x80808A08, 0x80809F75, 0x80809F75, 0x80800006, 0x80800006][i];
        let a = ss.array_range(sr + 0x98 + i * 16, stride, Some(sc))?;
        let b = ns.array_range(nr + 0x80 + i * 16, stride, Some(nc))?;
        ensure!(
            a.len() == sb.len() * stride && a.len() == b.len(),
            "Private skeleton parallel arrays differ"
        );
        if i == 0 {
            ensure!(
                ss.0[a.clone()]
                    .chunks_exact(16)
                    .zip(ns.0[b.clone()].chunks_exact(16))
                    .all(|(a, b)| a[4..] == b[4..]),
                "Private rig changes hierarchy links"
            );
        } else if i >= 3 {
            ensure!(
                ss.0[a.clone()] == ns.0[b.clone()],
                "Private rig changes FK/procedural slot assignments"
            );
        } else {
            ensure!(
                ss.0[a.clone()]
                    .chunks_exact(4)
                    .all(|v| f32::from_le_bytes(v.try_into().unwrap()).is_finite()),
                "Private rig has a nonfinite transform"
            );
        }
        ns.0[b].copy_from_slice(&ss.0[a]);
    }
    let sf = ss.array(si + 0x30, 24, Some(0x80800598))?;
    let nf = ns.array(ni + 0x30, 64, Some(0x80808544))?;
    ensure!(
        sf.len() == nf.len() && !sf.is_empty(),
        "Private FK section count differs"
    );
    for (&a, &b) in sf.iter().zip(&nf) {
        let source_poses = ss.array_range(a, 32, Some(0x808001B0))?;
        let native_poses = ns.array_range(b + 32, 32, Some(0x80809F75))?;
        ensure!(
            ss.0[source_poses] == ns.0[native_poses]
                && ss.u32(a + 16)? == ns.u32(b + 48)?
                && ss.u32(a + 20)? == 0,
            "Private rig needs another FK pose layout"
        );
    }
    let sp = ss.array(si + 0x40, 4, Some(0x808004B8))?;
    let np = ns.array(ni + 0x40, 8, Some(0x80808A06))?;
    ensure!(sp.len() == np.len(), "Private procedural ranges differ");
    for (&a, &b) in sp.iter().zip(&np) {
        for lane in 0..4 {
            ensure!(
                u16::from(ss.u8(a + lane)?) == ns.u16(b + lane * 2)?,
                "Private procedural range changes"
            );
        }
    }
    crate::tiger::animation::sections::validate_native_instance(&ns.0)?;
    crate::tiger::animation::sections::validate_source_relocation(&original.0, &ns.0, None)?;
    Ok((ns, changed_bones))
}
