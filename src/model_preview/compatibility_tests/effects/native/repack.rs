//! Package-to-GLB chart color witnesses prepared before coordinate repacking.
use super::*;

pub(in crate::model_preview::compatibility_tests) fn bytes<'a>(
    glb: &'a [u8],
    doc: &serde_json::Value,
    view: usize,
) -> &'a [u8] {
    let json_size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let view = &doc["bufferViews"][view];
    let start = 28 + json_size + view["byteOffset"].as_u64().unwrap_or(0) as usize;
    &glb[start..start + view["byteLength"].as_u64().unwrap() as usize]
}

pub(in crate::model_preview::compatibility_tests) fn png_pixels(
    png: &[u8],
) -> ([usize; 2], Vec<u8>) {
    let size = [16, 20].map(|at| u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize);
    let channels = match png[25] {
        2 => 3,
        6 => 4,
        other => panic!("Unexpected PNG color type {other}"),
    };
    let mut compressed = Vec::new();
    let mut cursor = 8;
    while cursor + 12 <= png.len() {
        let size = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            compressed.extend_from_slice(&png[cursor + 8..cursor + 8 + size]);
        }
        cursor += size + 12;
    }
    let mut raw = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::ZlibDecoder::new(compressed.as_slice()),
        &mut raw,
    )
    .unwrap();
    let pixels = raw
        .chunks_exact(size[0] * channels + 1)
        .flat_map(|row| {
            assert_eq!(row[0], 0);
            row[1..].chunks_exact(channels).flat_map(|pixel| {
                [
                    pixel[0],
                    pixel[1],
                    pixel[2],
                    if channels == 4 { pixel[3] } else { 255 },
                ]
            })
        })
        .collect();
    (size, pixels)
}

fn normal_basis(glb: &[u8], doc: &serde_json::Value, uv: &[[f32; 2]]) -> serde_json::Value {
    let primitive = &doc["meshes"][0]["primitives"][0];
    let read = |name: &str, lanes: usize| {
        let accessor = &doc["accessors"][primitive["attributes"][name].as_u64().unwrap() as usize];
        let bytes = bytes(glb, doc, accessor["bufferView"].as_u64().unwrap() as usize);
        (0..lanes)
            .map(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let normal = read("NORMAL", 3);
    let tangent = read("TANGENT", 4);
    assert_eq!(normal, [0.0, 0.0, 1.0]);
    assert_eq!(tangent, [0.0, 1.0, -0.0, 1.0]);
    let material = &doc["materials"][primitive["material"].as_u64().unwrap() as usize];
    let texture = material["normalTexture"]["index"].as_u64().unwrap() as usize;
    let image = &doc["images"][doc["textures"][texture]["source"].as_u64().unwrap() as usize];
    let png = bytes(glb, doc, image["bufferView"].as_u64().unwrap() as usize);
    let (size, pixels) = png_pixels(png);
    let center: [f32; 2] =
        std::array::from_fn(|axis| uv.iter().map(|p| p[axis]).sum::<f32>() / 3.0);
    let at = ((center[1] * size[1] as f32) as usize * size[0]
        + (center[0] * size[0] as f32) as usize)
        * 4;
    let packed: [u8; 3] = pixels[at..at + 3].try_into().unwrap();
    let mut map = packed.map(|v| f32::from(v) * 2.0 / 255.0 - 1.0);
    let length = map.iter().map(|v| v * v).sum::<f32>().sqrt();
    map = map.map(|v| v / length);
    let bitangent = [
        normal[1] * tangent[2] - normal[2] * tangent[1],
        normal[2] * tangent[0] - normal[0] * tangent[2],
        normal[0] * tangent[1] - normal[1] * tangent[0],
    ]
    .map(|v| v * tangent[3]);
    let actual: [f32; 3] = std::array::from_fn(|axis| {
        tangent[axis] * map[0] + bitangent[axis] * map[1] + normal[axis] * map[2]
    });
    // Native normal -Y, tangent +Z and handedness -1 produce bitangent +X.
    // Upright export maps that independently specified surface to [y, x, z].
    let x = 191.0 / 255.0 * 2.0 - 1.0;
    let y = 159.0 / 255.0 * 2.0 - 1.0;
    let expected = [y, x, (1.0_f32 - x * x - y * y).sqrt()];
    for axis in 0..3 {
        assert!(
            (actual[axis] - expected[axis]).abs() < 0.015,
            "{actual:?} != {expected:?}"
        );
    }
    json!({"tangent":tangent,"normal":normal,"packed":packed,"expected":expected,"actual":actual})
}

#[test]
fn varying_detail_survives_degenerate_and_repeated_primary_uvs_in_glb() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (name, primary) in [
        ("degenerate", [[0.25, 0.5]; 3]),
        ("repeated", [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]]),
    ] {
        let mut model = stored::opaque_detail_case();
        let corners = model.triangles[0];
        for (i, vertex) in corners.into_iter().enumerate() {
            model.uvs[vertex as usize] = primary[i];
            model.detail_uvs[vertex as usize] = [[0.0, 0.5], [1.0, 0.5], [0.0, 0.5]][i];
        }
        // The atlas's UV basis differs from the native normal basis. Keep that basis
        // explicit in the exported posed tangent accessor.
        model.tangents = vec![[0.0, 0.0, 1.0, -1.0]; model.vertices.len()];
        let normal_index = model.textures.len();
        model.textures.push(texture::Texture {
            mips: None,
            tag: 123,
            size: [1, 1],
            rgba: vec![191, 159, 255, 255],
            linear: None,
        });
        model.triangle_normals.fill(Some(normal_index));
        let glb = export::glb(&model, 0.0).unwrap();
        let json_size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_size]).unwrap();
        let primitive = &doc["meshes"][0]["primitives"][0];
        let accessor =
            &doc["accessors"][primitive["attributes"]["TEXCOORD_0"].as_u64().unwrap() as usize];
        assert_eq!(accessor["count"], 3);
        let uv_bytes = bytes(
            &glb,
            &doc,
            accessor["bufferView"].as_u64().unwrap() as usize,
        );
        let uv: Vec<[f32; 2]> = uv_bytes
            .chunks_exact(8)
            .map(|v| [0, 4].map(|i| f32::from_le_bytes(v[i..i + 4].try_into().unwrap())))
            .collect();
        let normal = normal_basis(&glb, &doc, &uv);
        let material = &doc["materials"][primitive["material"].as_u64().unwrap() as usize];
        let texture = material["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .unwrap() as usize;
        let image = &doc["images"][doc["textures"][texture]["source"].as_u64().unwrap() as usize];
        let png = bytes(&glb, &doc, image["bufferView"].as_u64().unwrap() as usize);
        let (size, pixels) = png_pixels(png);
        let colors: Vec<_> = [[0.625, 0.25, 0.125], [0.125, 0.75, 0.125]]
            .map(|weights| {
                let sample: [f32; 2] =
                    std::array::from_fn(|axis| (0..3).map(|i| uv[i][axis] * weights[i]).sum());
                let x = (sample[0] * size[0] as f32).floor() as usize;
                let y = (sample[1] * size[1] as f32).floor() as usize;
                let at = (y * size[0] + x) * 4;
                [pixels[at], pixels[at + 1], pixels[at + 2]]
            })
            .into();
        assert!(
            colors[0][0] > colors[0][2].saturating_add(100),
            "Missing red landmark: {colors:?}"
        );
        assert!(
            colors[1][2] > colors[1][0].saturating_add(100),
            "Missing blue landmark: {colors:?}"
        );
        std::fs::write(output.join(format!("repacked-{name}.glb")), &glb).unwrap();
        std::fs::write(output.join(format!("repacked-{name}.png")), png).unwrap();
        receipt.push(json!({"case":name,"primary_uv":primary,"uv":uv,"size":size,"colors":colors,"normal_basis":normal}));
    }
    std::fs::write(
        output.join("repacked-detail-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
