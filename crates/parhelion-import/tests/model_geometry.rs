//! Real source models through the public geometry exporter, with independent OBJ readback.
use anyhow::{Context, Result, ensure};
use parhelion_import::d2_mot::{
    geometry,
    payload::Payload,
    reader::{self, Reader},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires modern packages, PARHELION_MODEL_GEOMETRY_CASES and fresh PARHELION_MODEL_GEOMETRY_OUTPUT"]
fn source_triangle_lists_export_exact_geometry_without_false_skinning() -> Result<()> {
    let packages = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let output = reader::outside(
        &configured("PARHELION_MODEL_GEOMETRY_OUTPUT")?,
        packages.parent().context("Source root")?,
    )?;
    ensure!(!output.exists(), "Use a fresh geometry artifact directory");
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_MODEL_GEOMETRY_CASES")?)?)?;
    ensure!(!cases.is_empty(), "Configure source geometry");
    let mut source = Reader::new(&packages, &output, true)?;
    let mut receipt = Vec::new();
    for case in cases {
        let tag = u32::try_from(case["model"].as_u64().context("Model tag")?)?;
        let index = usize::try_from(case["mesh"].as_u64().context("Mesh index")?)?;
        let model = source.tag(tag, Some(0x80806F07))?;
        let mesh = model.array(16, 128, Some(0x80806EC5))?[index];
        let mut read = |offset| -> Result<(Payload, Payload)> {
            let header = model.u32(mesh + offset)?;
            Ok((
                (*source.tag(header, None)?).clone(),
                (*source.tag(source.reference(header)?, None)?).clone(),
            ))
        };
        let (ph, pos) = read(0)?;
        let (_, uv) = read(4)?;
        let (ih, indices) = read(16)?;
        ensure!(
            (ph.u16(4)?, ph.u16(6)?) == (24, 0),
            "Configure unskinned packed geometry"
        );
        let wide = ih.u8(1)? != 0;
        let mut used = BTreeSet::new();
        let mut expected_faces = Vec::new();
        let mut ranges = BTreeSet::new();
        for part in model.array(mesh + 32, 36, Some(0x80806ECB))? {
            let start = model.u32(part + 8)? as usize;
            let count = model.u32(part + 12)? as usize;
            if model.u8(part + 29)? > 3 || !ranges.insert((start, count)) {
                continue;
            }
            ensure!(
                model.u16(part + 6)? == 3 && count.is_multiple_of(3),
                "Expected triangle list"
            );
            for index in (start..start + count).step_by(3) {
                let mut face = [0usize; 3];
                for (lane, vertex) in face.iter_mut().enumerate() {
                    *vertex = if wide {
                        indices.u32((index + lane) * 4)? as usize
                    } else {
                        usize::from(indices.u16((index + lane) * 2)?)
                    };
                }
                if face[0] == face[1] || face[1] == face[2] || face[0] == face[2] {
                    continue;
                }
                used.extend(face);
                expected_faces.push(face);
            }
        }
        ensure!(!expected_faces.is_empty(), "Empty reference mesh");
        let report = geometry::export_mesh(&mut source, tag, index, false, geometry::Detail::Full)?;
        ensure!(
            report["rigid_bone_zero"] == true,
            "Homogeneous position W became a bone selector"
        );
        ensure!(
            report["full_detail_triangles"].as_u64() == Some(expected_faces.len() as u64),
            "Triangles were lost or duplicated"
        );
        let text = fs::read_to_string(output.join(report["obj"].as_str().context("OBJ file")?))?;
        let vectors = |prefix: &str| -> Result<Vec<Vec<f32>>> {
            text.lines()
                .filter_map(|line| line.strip_prefix(prefix))
                .map(|line| {
                    line.split_whitespace()
                        .map(|v| Ok(v.parse::<f32>()?))
                        .collect()
                })
                .collect()
        };
        let positions = vectors("v ")?;
        let normals = vectors("vn ")?;
        let uvs = vectors("vt ")?;
        ensure!(
            positions.len() == used.len() && normals.len() == used.len() && uvs.len() == used.len(),
            "OBJ vertex channels differ"
        );
        let mut maximum_error = 0f32;
        let mut mapped = BTreeMap::new();
        for (out, &vertex) in used.iter().enumerate() {
            mapped.insert(vertex, out + 1);
            ensure!(
                pos.i16(vertex * 24 + 6)? == 32767,
                "Source homogeneous W changed"
            );
            for axis in 0..3 {
                let expected = pos.i16(vertex * 24 + axis * 2)? as f32 / 32767.
                    * model.f32(0x50 + axis * 4)?
                    + model.f32(0x60 + axis * 4)?;
                maximum_error = maximum_error.max((positions[out][axis] - expected).abs());
                ensure!(
                    (positions[out][axis] - expected).abs() < 1e-5,
                    "Position changed"
                );
                ensure!(
                    (normals[out][axis] - pos.i16(vertex * 24 + 8 + axis * 2)? as f32 / 32767.)
                        .abs()
                        < 1e-6,
                    "Normal changed"
                );
            }
            for (axis, actual) in uvs[out].iter().enumerate().take(2) {
                let expected = uv.i16(vertex * 4 + axis * 2)? as f32 / 32767.
                    * model.f32(0x70 + axis * 4)?
                    + model.f32(0x78 + axis * 4)?;
                ensure!((actual - expected).abs() < 1e-5, "UV changed");
            }
        }
        let exported = text
            .lines()
            .filter_map(|line| line.strip_prefix("f "))
            .map(|line| {
                line.split_whitespace()
                    .map(|v| Ok(v.split('/').next().context("Face")?.parse::<usize>()?))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            exported
                == expected_faces
                    .iter()
                    .map(|f| f.iter().map(|v| mapped[v]).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            "Winding or vertex selection changed"
        );
        receipt.push(json!({"model":tag,"mesh":index,"vertices":used.len(),"triangles":expected_faces.len(),"maximum_position_error":maximum_error,"export":report,"materials_verified":false}));
    }
    source.finish()?;
    reader::write_json(
        &output.join("geometry-readback.json"),
        &json!({"cases":receipt,"installed":false,"gameplay_verified":false}),
    )
}
