//! Package-to-render and baked detail landmarks for the executed native vertex formula.
use super::*;
use effects::native::repack;
mod budget;
mod fixture;

fn recovered(mode: u8, panel: usize) -> bool {
    mode <= 2 || mode == 10 || mode == 9 && panel != 1
}

fn model(mode: u8, reference: bool) -> Model {
    let (directory, tag) = fixture::build(mode);
    let mut model = fixtures::load(directory.path(), tag).unwrap();
    let detail = model.textures.len();
    for normal in [false, true] {
        let rgba = (0..4)
            .flat_map(|y| {
                (0..4).flat_map(move |x| {
                    if normal {
                        [96 + x * 24, 80 + y * 36, 200, 255]
                    } else {
                        [40 + x * 60, 24 + y * 64, 208 - x * 32, 255]
                    }
                })
            })
            .collect();
        model.textures.push(texture::Texture {
            mips: None,
            tag: 0x8080_ff00 + u32::from(normal),
            size: [4, 4],
            rgba,
            linear: None,
        });
    }
    model.dyes[0] = Some(shader::Dye {
        surface: crate::dyes::material::Surface {
            albedo: [0.4, 0.55, 0.7],
            worn_albedo: [0.4, 0.55, 0.7],
            params: [1.0, 0.75, 0.0, 0.0],
            worn_params: [1.0, 0.75, 0.0, 0.0],
            roughness: [0.0, 0.0, 0.4, 0.0],
            worn_roughness: [0.0, 0.0, 0.4, 0.0],
            wear: [0.0, 1.0, 0.0, 1.0],
            emissive: [0.0; 3],
            iridescence: -1.0,
        },
        detail: Some(detail),
        normal: Some(detail + 1),
        transform: [if mode == 10 { 64.0 } else { 1.0 }, 1.0, 0.0, 0.0],
        normal_transform: [if mode == 10 { 64.0 } else { 1.0 }, 1.0, 0.0, 0.0],
        vectors: [[0.0; 4]; 27],
    });
    if reference {
        for panel in 0..3 {
            model.triangle_detail_uv[panel * 2..panel * 2 + 2].fill(recovered(mode, panel));
            if recovered(mode, panel) {
                let primary: [f32; 2] = std::array::from_fn(|i| {
                    fixture::UV[panel][i] * fixture::PLACEMENT[i] + fixture::PLACEMENT[i + 2]
                });
                for (corner, index) in model.detail_uvs[panel * 4..panel * 4 + 4]
                    .iter_mut()
                    .enumerate()
                {
                    *index = std::array::from_fn(|i| {
                        primary[i] * fixture::auxiliary(mode, panel, corner)[i]
                    });
                }
            }
        }
    }
    model
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn cases() -> Vec<(String, Model)> {
    (0..=9)
        .map(|mode| (format!("native-placed-detail-{mode}"), model(mode, false)))
        .collect()
}

fn embedded(glb: &[u8]) -> Vec<Vec<u8>> {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    doc["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|image| {
            let view = image["bufferView"].as_u64().unwrap() as usize;
            repack::png_pixels(repack::bytes(glb, &doc, view)).1
        })
        .collect()
}

#[test]
fn native_placed_detail_survives_sparse_draws_rendering_and_baking() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for mode in 0..=9 {
        let actual = model(mode, false);
        let reference = model(mode, true);
        let name = format!("native-placed-detail-{mode}");
        let camera = render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        };
        let images = [&actual, &reference].map(|m| render::image(m, camera, [480, 240]));
        for (suffix, image) in [("actual", &images[0]), ("reference", &images[1])] {
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            std::fs::write(
                out.join(format!("{name}-{suffix}.png")),
                export::png(&rgba, 480, 240).unwrap(),
            )
            .unwrap();
        }
        let maximum = images[0]
            .pixels
            .iter()
            .zip(&images[1].pixels)
            .flat_map(|(a, b)| {
                a.to_array()
                    .into_iter()
                    .zip(b.to_array())
                    .map(|(a, b)| a.abs_diff(b))
            })
            .max()
            .unwrap();
        let actual_glb = export::glb(&actual, 0.0).unwrap();
        let reference_glb = export::glb(&reference, 0.0).unwrap();
        std::fs::write(out.join(format!("{name}.glb")), &actual_glb).unwrap();
        std::fs::write(out.join(format!("{name}-reference.glb")), &reference_glb).unwrap();
        let flags: Vec<_> = (0..3).map(|panel| recovered(mode, panel)).collect();
        receipt.push(json!({"mode":mode,"placement":fixture::PLACEMENT,"raw_uv":fixture::UV,"auxiliary":fixture::AUX,"expected_recovered":flags,"actual_recovered":actual.triangle_detail_uv,"secondary":actual.detail_uvs,"maximum_channel_error":maximum,"notices":actual.notices}));
        std::fs::write(
            out.join("native-placed-detail.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        assert!(maximum <= 1, "{name}: render error {maximum}");
        assert_eq!(
            embedded(&actual_glb),
            embedded(&reference_glb),
            "{name}: baked color or normal landmarks"
        );
        for panel in 0..3 {
            assert_eq!(
                actual.triangle_detail_uv[panel * 2],
                recovered(mode, panel),
                "{name} panel {panel}"
            );
        }
    }
}
