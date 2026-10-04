use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
};

/// Which detail categories of a source model count as part of the weapon.
///
/// The engine's level-of-detail enumeration calls category 0 main geometry, 1
/// grip and stock, 2 stickers and 3 internal geometry. All four are full
/// quality; categories 4, 7, 8 and 9 are reduced-detail copies that the native
/// model rebuilds from the retained parts, so they are never imported.
///
/// `Full` recovers grips, stocks and scopes, which are otherwise dropped along
/// with any model made only of them. Some of those models carry materials the
/// converter cannot translate yet, and a weapon is more useful without its grip
/// than not imported at all, so `Main` reproduces the older behaviour for them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Detail {
    #[default]
    Full,
    Main,
}

impl Detail {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("full") => Ok(Self::Full),
            Some("main") => Ok(Self::Main),
            Some(other) => anyhow::bail!("unknown source detail level {other}"),
        }
    }

    fn keeps(self, category: u8) -> bool {
        match self {
            Self::Full => matches!(category, 0..=3),
            Self::Main => category == 0,
        }
    }
}
pub fn triangles(indices: &[u32], vertices: usize, restart: u32) -> Result<Vec<[u32; 3]>> {
    let mut window = Vec::new();
    let mut parity = false;
    let mut faces = vec![];
    for (at, &i) in indices.iter().enumerate() {
        if at.is_multiple_of(4096) {
            crate::cancellation::check()?;
        }
        if i == restart {
            window.clear();
            parity = false;
            continue;
        }
        ensure!((i as usize) < vertices, "missing vertex {i}");
        window.push(i);
        if window.len() < 3 {
            continue;
        }
        let mut face = [window[0], window[1], window[2]];
        if parity {
            face.swap(0, 1)
        }
        parity = !parity;
        if face[0] != face[1] && face[1] != face[2] && face[0] != face[2] {
            faces.push(face)
        }
        window.remove(0);
    }
    Ok(faces)
}
pub fn export(r: &mut Reader, tag: u32, detail: Detail) -> Result<Value> {
    export_mesh(r, tag, 0, true, detail)
}

/// Resolve an exported mesh without silently selecting the first mesh of a model.
/// Older exports omitted the index and are valid only for single-mesh models.
pub(crate) fn selected_mesh(
    model: &crate::d2_mot::payload::Payload,
    entry: &Value,
) -> Result<usize> {
    let meshes = model.array(16, 128, Some(0x80806EC5))?;
    let index = match entry.get("mesh_index") {
        Some(value) => usize::try_from(value.as_u64().context("source mesh index")?)?,
        None => {
            ensure!(
                meshes.len() == 1,
                "source export lacks a mesh index for a multi-mesh model"
            );
            0
        }
    };
    meshes
        .get(index)
        .copied()
        .context("source mesh index outside model")
}

pub(crate) fn mesh_key(entry: &Value) -> Result<String> {
    let tag = u32::from_str_radix(entry["model"].as_str().context("source model tag")?, 16)?;
    Ok(match entry.get("mesh_index") {
        Some(index) => format!(
            "{tag:08X}-mesh-{}",
            index.as_u64().context("source mesh index")?
        ),
        None => format!("{tag:08X}"),
    })
}

pub(crate) fn indices(header: &Payload, data: &Payload) -> Result<(Vec<u32>, u32)> {
    let wide = header.u8(1)? != 0;
    let width = if wide { 4 } else { 2 };
    ensure!(
        data.0.len() == header.u32(8)? as usize && data.0.len().is_multiple_of(width),
        "Index payload size differs from its header"
    );
    let values = (0..data.0.len())
        .step_by(width)
        .map(|at| {
            if wide {
                data.u32(at)
            } else {
                Ok(u32::from(data.u16(at)?))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((values, if wide { u32::MAX } else { u32::from(u16::MAX) }))
}
pub fn export_mesh(
    r: &mut Reader,
    tag: u32,
    index: usize,
    single: bool,
    detail: Detail,
) -> Result<Value> {
    let model = r.tag(tag, Some(0x80806F07))?;
    let meshes = model.array(16, 128, Some(0x80806EC5))?;
    ensure!(!single || meshes.len() == 1, "expected single mesh");
    let mesh = *meshes
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("mesh index outside model"))?;
    let auxiliary = model.u32(mesh + 24)?;
    if auxiliary != 0 && auxiliary != u32::MAX {
        r.tag(auxiliary, None)?;
        r.tag(r.reference(auxiliary)?, None)?;
    }
    let pt = model.u32(mesh)?;
    let ut = model.u32(mesh + 4)?;
    let it = model.u32(mesh + 16)?;
    let ph = r.tag(pt, None)?;
    let uh = r.tag(ut, None)?;
    ensure!(
        (ph.i16(4)?, ph.i16(6)?) == (24, 1) && (uh.i16(4)?, uh.i16(6)?) == (4, 1),
        "unsupported packed vertex format"
    );
    let pos = r.tag(r.reference(pt)?, None)?;
    let uv = r.tag(r.reference(ut)?, None)?;
    ensure!(
        pos.0.len() == ph.u32(0)? as usize && uv.0.len() == uh.u32(0)? as usize,
        "vertex header size mismatch"
    );
    ensure!(
        pos.0.len() % 24 == 0 && uv.0.len() == pos.0.len() / 24 * 4,
        "vertex counts differ"
    );
    let vertices = pos.0.len() / 24;
    let ih = r.tag(it, None)?;
    let ib = r.tag(r.reference(it)?, None)?;
    let (indices, restart) = indices(&ih, &ib)?;
    let mut seen = BTreeSet::new();
    let mut parts = vec![];
    for part in model.array(mesh + 32, 36, Some(0x80806ECB))? {
        let start = model.u32(part + 8)? as usize;
        let count = model.u32(part + 12)? as usize;
        if !detail.keeps(model.u8(part + 29)?) || !seen.insert((start, count)) {
            continue;
        }
        ensure!(model.u16(part + 6)? == 5, "unsupported primitive");
        let slice = indices
            .get(start..start + count)
            .ok_or_else(|| anyhow::anyhow!("part index range exceeds buffer"))?;
        parts.push((model.u32(part)?, triangles(slice, vertices, restart)?));
    }
    let used = parts
        .iter()
        .flat_map(|(_, f)| f.iter().flatten().copied())
        .collect::<BTreeSet<_>>();
    ensure!(!used.is_empty(), "no full-detail geometry");
    let mapped = used
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, i + 1))
        .collect::<BTreeMap<_, _>>();
    let filename = if single {
        format!("{tag:08X}-lod0.obj")
    } else {
        format!("{tag:08X}-mesh-{index}-lod0.obj")
    };
    let mut out = std::io::BufWriter::new(File::create(r.output.join(&filename))?);
    writeln!(
        out,
        "# Modern source geometry; not a Shadowkeep-ready model."
    )?;
    for &i in &used {
        write!(out, "v")?;
        for a in 0..3 {
            write!(
                out,
                " {}",
                pos.i16(i as usize * 24 + a * 2)? as f32 / 32767. * model.f32(0x50 + a * 4)?
                    + model.f32(0x60 + a * 4)?
            )?
        }
        writeln!(out)?
    }
    for &i in &used {
        write!(out, "vt")?;
        for a in 0..2 {
            write!(
                out,
                " {}",
                uv.i16(i as usize * 4 + a * 2)? as f32 / 32767. * model.f32(0x70 + a * 4)?
                    + model.f32(0x78 + a * 4)?
            )?
        }
        writeln!(out)?
    }
    for &i in &used {
        write!(out, "vn")?;
        for a in 4..7 {
            write!(
                out,
                " {}",
                pos.i16(i as usize * 24 + a * 2)? as f32 / 32767.
            )?
        }
        writeln!(out)?
    }
    for (n, (mat, faces)) in parts.iter().enumerate() {
        writeln!(out, "g part_{n}_material_{mat:08X}")?;
        for face in faces {
            write!(out, "f")?;
            for i in face {
                let m = mapped[i];
                write!(out, " {m}/{m}/{m}")?
            }
            writeln!(out)?
        }
    }
    out.flush()?;
    let mut bones = BTreeMap::new();
    for o in (0..pos.0.len()).step_by(24) {
        *bones.entry(pos.i16(o + 6)?.to_string()).or_insert(0usize) += 1;
    }
    Ok(
        json!({"model":format!("{tag:08X}"),"mesh_index":index,"obj":filename,"vertices_all_lods":vertices,"full_detail_vertices":used.len(),"full_detail_triangles":parts.iter().map(|(_,f)|f.len()).sum::<usize>(),"rigid_bone_zero":bones.len()==1&&bones.contains_key("0"),"bone_selectors":bones,"materials":model.array(mesh+32,36,Some(0x80806ECB))?.iter().map(|&p|Ok(format!("{:08X}",model.u32(p)?))).collect::<Result<BTreeSet<_>>>()?}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strip_restart_and_degenerates() {
        assert_eq!(
            triangles(&[0, 1, 2, 3, 65535, 4, 4, 5, 6], 7, 65535).unwrap(),
            vec![[0, 1, 2], [2, 1, 3], [5, 4, 6]]
        );
        assert_eq!(
            triangles(&[0, 1, 2, 3, 65535, 4, 5, 6], 7, 65535).unwrap(),
            vec![[0, 1, 2], [2, 1, 3], [4, 5, 6]]
        );
        assert_eq!(
            triangles(&[0, 1, 1, 2, 3], 4, 65535).unwrap(),
            vec![[1, 2, 3]]
        );
        assert!(triangles(&[9], 3, 65535).is_err());
    }
}
