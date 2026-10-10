//! Native material reads and both renderers, with closed-form light and coverage oracles.
//! Written before the effect implementation. See the dated material investigation for failures.
use super::*;
use crate::model_preview::effects::{Kind, Material};
use fixtures::{Package, array, floats, put};
mod glow;
pub(super) mod native;
pub(crate) mod transparency;
pub(in crate::model_preview) use native::motion_case;
pub(crate) use native::opaque_detail_case;
#[cfg_attr(
    not(windows),
    allow(unused_imports, reason = "used by the Windows GPU verification")
)]
pub(crate) use native::{
    derivative, hdr, immediate, integer, layered, metal, normal_blue, normals, opaque, paint,
    vertex_image,
};

fn gradient() -> Material {
    Material {
        kind: Kind::Gradient,
        constants: vec![
            [1.0, 0.0, 0.0, 0.0],
            [1.0; 4],
            [0.125, 0.0625, 0.03125, 1.0],
            [0.0; 4],
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0; 4],
            [1.0; 4],
            [1.0; 4],
        ],
        ..Default::default()
    }
}

fn quad(model: &mut Model, depth: f32, effect: Option<usize>, constant: Option<[f32; 3]>) {
    let base = model.vertices.len() as u32;
    model.vertices.extend([
        [-1.0, depth, -1.0],
        [1.0, depth, -1.0],
        [1.0, depth, 1.0],
        [-1.0, depth, 1.0],
    ]);
    model.normals.extend([[0.0, -1.0, 0.0]; 4]);
    model.uvs.extend([[0.5, 0.5]; 4]);
    model
        .triangles
        .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    model.triangle_effects.extend([effect; 2]);
    model.triangle_constant.extend([constant; 2]);
}

fn encoded(linear: [f32; 3]) -> [u8; 3] {
    linear.map(|v| {
        let v = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (255.0 * v).round().clamp(0.0, 255.0) as u8
    })
}

pub(crate) fn render_cases() -> Vec<(String, Model, [u8; 3])> {
    let mut cases = Vec::new();
    for reverse in [false, true] {
        for depth in [-0.1, 0.0, 0.1] {
            let mut model = Model::default();
            model.effects.push(gradient());
            if reverse {
                quad(&mut model, depth, Some(0), None);
                quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
            } else {
                quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
                quad(&mut model, depth, Some(0), None);
            }
            // Two coplanar triangles must shade their shared diagonal exactly once.
            let expected = if depth > 0.0 {
                [0.0, 0.0, 0.125]
            } else {
                [0.125, 0.0625, 0.15625]
            };
            cases.push((
                format!("effect-depth-{depth}-{reverse}"),
                model,
                encoded(expected),
            ));
        }
    }
    for (name, intensity, grazing, uv_edge) in [
        ("overlap", 1.0, false, false),
        ("inactive", 0.0, false, false),
        ("grazing", 1.0, true, false),
        ("uv-edge", 1.0, false, true),
    ] {
        let mut model = Model::default();
        let mut effect = gradient();
        effect.constants[8] = [intensity; 4];
        if grazing {
            effect.constants[4] = [1.0, 0.0, 0.0, 0.0];
        }
        if uv_edge {
            effect.constants[5] = [1.0, -2.0, -0.5, 0.0];
        }
        model.effects.push(effect);
        quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
        quad(&mut model, -0.1, Some(0), None);
        quad(&mut model, -0.2, Some(0), None);
        if grazing {
            model.normals[4..].fill([1.0, 0.0, 0.0]);
        }
        if uv_edge {
            model.uvs[4..].fill([0.0, 0.0]);
        }
        let expected = if name == "overlap" {
            [0.25, 0.125, 0.1875]
        } else {
            [0.0, 0.0, 0.125]
        };
        cases.push((format!("effect-{name}"), model, encoded(expected)));
    }
    form_cases(&mut cases);
    glow::render_cases(&mut cases);
    native::render_cases(&mut cases);
    cases
}

fn form_cases(cases: &mut Vec<(String, Model, [u8; 3])>) {
    for kind in [
        Kind::SoftGradient,
        Kind::GearGlow,
        Kind::GearFresnel,
        Kind::ScrollingMasks,
    ] {
        let mut material = Material {
            kind,
            constants: vec![[0.0; 4]; 41],
            ..Default::default()
        };
        let c = &mut material.constants;
        let light = [0.125, 0.0625, 0.03125, 1.0];
        match kind {
            Kind::SoftGradient => {
                c[0] = [0.0, 1.0, 0.0, 0.0];
                c[1] = [1.0, 0.0, 0.0, 0.0];
                c[2] = [1.0; 4];
                c[3] = light;
                c[5] = [1.0, 0.0, 0.0, 0.0];
                c[6] = c[5];
                c[7] = [0.0, 1.0, 0.0, 0.0];
                c[8] = [1.0; 4];
                c[9] = [1.0; 4];
            }
            Kind::GearGlow => {
                c[4] = light;
                c[6] = [1.0; 4];
                c[7] = [1.0; 4];
            }
            Kind::GearFresnel => {
                c[4] = [1.0; 4];
                c[5] = light;
                c[8] = [1.0; 4];
                c[14] = [1.0; 4];
                c[15] = [1.0; 4];
                c[17] = [0.0, 1.0, 0.0, 0.0];
                c[18] = [1.0; 4];
                c[19] = [1.0; 4];
                c[20] = [1.0; 4];
            }
            Kind::ScrollingMasks => {
                c[34] = light;
                c[35] = [1.0; 4];
                c[37] = [1.0; 4];
                c[38] = [1.0; 4];
                c[39] = [0.5; 4];
                c[40] = [1.0; 4];
            }
            _ => unreachable!(),
        }
        let mut model = Model::default();
        let mask = kind == Kind::ScrollingMasks;
        model.textures.push(texture::Texture {
            mips: None,
            linear: None,
            tag: 1,
            size: [1, 1],
            rgba: if mask {
                vec![255; 4]
            } else {
                vec![0, 0, 0, 255]
            },
        });
        model.textures.push(texture::Texture {
            mips: None,
            linear: None,
            tag: 2,
            size: [1, 1],
            rgba: if mask { vec![255; 4] } else { vec![0; 4] },
        });
        model.effects.push(material);
        quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
        quad(&mut model, -0.1, Some(0), None);
        model.triangle_textures = vec![None, None, Some(0), Some(0)];
        model.triangle_gearstacks = vec![None, None, Some(1), Some(1)];
        cases.push((
            format!("effect-form-{kind:?}"),
            model,
            encoded([0.125, 0.0625, if mask { 0.09375 } else { 0.15625 }]),
        ));
    }
}

#[test]
fn effects_blend_light_without_opaque_cards_or_depth_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let output = crate::test_support::artifacts("effects");
    let output = output.as_deref().unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (name, model, expected) in render_cases() {
        let image = render::styled_image(
            &model,
            render::Camera {
                yaw: 0.0,
                pitch: 0.0,
                ..Default::default()
            },
            render::Scene::unprocessed(),
            [160, 160],
            0.0,
            render::Style::Textured,
        );
        let mut error = 0;
        for y in 72..88 {
            for x in 72..88 {
                for (actual, expected) in image.pixels[y * 160 + x].to_array()[..3]
                    .iter()
                    .zip(expected)
                {
                    error = error.max(actual.abs_diff(expected));
                }
            }
        }
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let png = export::png(&rgba, 160, 160).unwrap();
        assert_eq!(eframe::icon_data::from_png_bytes(&png).unwrap().rgba, rgba);
        std::fs::write(output.join(format!("{name}.png")), png).unwrap();
        receipt.push(json!({"case":name,"expected":expected,"maximum_error":error}));
        assert!(error <= 2, "{name}: {error}");
    }
    std::fs::write(
        output.join("effects.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn material_program_and_render_stage_survive_package_loading() {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    // A typed shader envelope identifies a captured program family. No copyrighted shader
    // bytecode is distributed in fixtures. Native packages separately verify the full shader.
    let mut dxbc = vec![0; 52];
    dxbc[..4].copy_from_slice(b"DXBC");
    dxbc[4..20].copy_from_slice(&[
        0x4d, 0xf8, 0x1e, 0x31, 0x82, 0x3a, 0xc6, 0x8a, 0x45, 0xf8, 0x8d, 0x48, 0x57, 0x13, 0x23,
        0xaf,
    ]);
    put(&mut dxbc, 24, &52u32.to_le_bytes());
    put(&mut dxbc, 28, &1u32.to_le_bytes());
    put(&mut dxbc, 32, &36u32.to_le_bytes());
    dxbc[36..40].copy_from_slice(b"SHEX");
    put(&mut dxbc, 40, &8u32.to_le_bytes());
    put(&mut dxbc, 44, &0x50u32.to_le_bytes());
    put(&mut dxbc, 48, &2u32.to_le_bytes());
    let header = package.raw(0, 33, 0, vec![0; 40]);
    let payload = package.raw(header, 41, 0, dxbc);
    package.set_reference(header, payload);
    let mut material = vec![0; 0x3a0];
    material[0x20] = 0x88;
    put(&mut material, 0x2c8, &header.to_le_bytes());
    let mut constants = gradient().constants;
    constants[8] = [0.0; 4];
    array(
        &mut material,
        0x318,
        0x80800090,
        &constants
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
        16,
    );
    array(&mut material, 0x2e8, 0x80800009, &[0x4D, 0, 0x43, 8], 1);
    array(
        &mut material,
        0x2f8,
        0x80800090,
        &[1.0f32; 4]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
        16,
    );
    let material = package.add(0x808071E8, material);
    let vertices = [
        [-1.0f32, 0.0, -1.0, 0.5, 0.5, 0.0, -1.0, 0.0],
        [1.0, 0.0, -1.0, 0.5, 0.5, 0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0, 0.5, 0.5, 0.0, -1.0, 0.0],
    ];
    let vertex = package.vertex(
        32,
        vertices
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect(),
    );
    let indices = package.indices(false);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x50, &[1.0; 3]);
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mesh = array(&mut model, 0x10, 0x80807378, &[0; 0x88], 0x88);
    put(&mut model, mesh, &vertex.to_le_bytes());
    put(&mut model, mesh + 0x10, &indices.to_le_bytes());
    for stage in 0..24 {
        put(
            &mut model,
            mesh + 0x28 + stage * 2,
            &(if stage <= 7 { 0i16 } else { 1 }).to_le_bytes(),
        );
    }
    put(&mut model, mesh + 0x58 + 7 * 2, &13u16.to_le_bytes());
    let mut part = [0; 32];
    put(&mut part, 0, &material.to_le_bytes());
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    part[0x1a] = 255;
    array(&mut model, mesh + 0x18, 0x8080737E, &part, 32);
    let tag = package.add(MODEL, model);
    // Cached channel vectors follow declaration order, not the numeric-storage selector.
    // The second declaration has no numeric slot but still supplies a stored initial vector.
    let mut bank = vec![0; 0x400];
    put(&mut bank, 0x10, &0x30i64.to_le_bytes());
    put(&mut bank, 0x18, &0x1E8i64.to_le_bytes());
    put(&mut bank, 0x3C, &0x8080979Fu32.to_le_bytes());
    put(&mut bank, 0x1FC, &0x80809790u32.to_le_bytes());
    let vectors = [0.25f32; 4]
        .into_iter()
        .chain([1.0; 4])
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    array(&mut bank, 0x40 + 0x50, 0x80800090, &vectors, 16);
    let mut declarations = [0; 224];
    put(&mut declarations, 0, &11u32.to_le_bytes());
    put(&mut declarations, 112, &22u32.to_le_bytes());
    put(&mut declarations, 112 + 72, &u16::MAX.to_le_bytes());
    array(&mut bank, 0x200 + 0xD8, 0x808097A1, &declarations, 112);
    let bank = package.add(RESOURCE, bank);
    let mut owner = vec![0; 0x600];
    put(&mut owner, 0x10, &0x30i64.to_le_bytes());
    put(&mut owner, 0x18, &0x1E8i64.to_le_bytes());
    put(&mut owner, 0x3C, &0x808072B8u32.to_le_bytes());
    put(&mut owner, 0x1FC, &0x808072BDu32.to_le_bytes());
    put(&mut owner, 0x200 + 0x1DC, &tag.to_le_bytes());
    let row = array(&mut owner, 0x40 + 0x120, 0x80809788, &[0; 96], 96);
    let link = owner.len();
    owner.resize(link + 40, 0);
    put(&mut owner, row + 4, &0x80809789u32.to_le_bytes());
    put(&mut owner, row + 8, &(link as u64).to_le_bytes());
    put(&mut owner, link + 4, &0x80809788u32.to_le_bytes());
    put(&mut owner, link + 8, &(row as u64).to_le_bytes());
    put(&mut owner, link + 24, &0x808097C1u64.to_le_bytes());
    put(&mut owner, link + 32, &22u32.to_le_bytes());
    let owner = package.add(RESOURCE, owner);
    let mut entity = vec![0; 0x28];
    let mut components = [0; 24];
    put(&mut components, 0, &owner.to_le_bytes());
    put(&mut components, 12, &bank.to_le_bytes());
    array(&mut entity, 0x10, 0x80809C04, &components, 12);
    let tag = package.add(ENTITY, entity);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let model = load_with_manager(&manager, tag, &Load::default(), None).unwrap();
    assert!(model.triangle_constant.iter().all(Option::is_none));
    assert_eq!(model.effects.len(), 1, "{:?}", model.notices);
    let image = render::styled_image(
        &model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene {
            filmic: false,
            bloom: false,
            background: [0; 3],
            ..render::Scene::unit_exposure()
        },
        [160, 160],
        0.0,
        render::Style::Textured,
    );
    let actual = image.pixels[80 * 160 + 80].to_array();
    let expected = encoded([0.125, 0.0625, 0.03125]);
    assert!(
        actual[..3]
            .iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "{actual:?}"
    );
    if let Some(output) =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string)
    {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        std::fs::write(
            output.join("packaged-effect.png"),
            export::png(&rgba, 160, 160).unwrap(),
        )
        .unwrap();
        std::fs::write(
            output.join("packaged-effect.json"),
            serde_json::to_vec_pretty(
                &json!({"actual":actual,"expected":expected,"notices":model.notices}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn red_dwarf_materials_render_from_installed_appearance() {
    let packages = crate::test_support::preview_packages();
    let output = crate::test_support::artifact_dir("effects");
    std::fs::create_dir_all(&output).unwrap();
    let appearance = appearance::Appearance {
        arrangement: 900,
        dyes: vec![(4, 3702), (5, 3703), (6, 3704)],
        dye_textures: vec![],
    };
    let model = appearance::load(&packages, &appearance).unwrap();
    assert!(model.effects.len() >= 6, "{:?}", model.notices);
    assert!(model.triangle_constant.iter().all(Option::is_none));
    assert!(
        !model
            .notices
            .iter()
            .any(|n| n.starts_with("Effect material")),
        "{:?}",
        model.notices
    );
    let mut captures = Vec::new();
    for (name, yaw, pitch, seconds) in [
        ("side", 0.0, 0.0, 0.0),
        ("perspective", 0.6, 0.25, 0.0),
        ("later", 0.6, 0.25, 2.0),
    ] {
        let camera = render::Camera {
            yaw,
            pitch,
            ..Default::default()
        };
        let image = render::styled_image(
            &model,
            camera,
            render::Scene::unprocessed(),
            [1280, 960],
            seconds,
            render::Style::Textured,
        );
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        let filename = format!("red-dwarf-{name}.png");
        std::fs::write(
            output.join(&filename),
            export::png(&rgba, 1280, 960).unwrap(),
        )
        .unwrap();
        captures.push(filename);
    }
    std::fs::write(output.join("red-dwarf.json"),serde_json::to_vec_pretty(&json!({"arrangement":900,"captures":captures,"effects":model.effects.len(),"notices":model.notices,"gameplay_verified":false})).unwrap()).unwrap();
}
