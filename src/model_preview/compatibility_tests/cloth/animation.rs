use super::*;

pub(super) fn components(package: &mut Package) -> [u32; 2] {
    let mut skeleton = resource(0x8080_8546);
    let mut hierarchy = [0; 16];
    put(&mut hierarchy, 4, &(-1i32).to_le_bytes());
    array(&mut skeleton, 0x100, 0x8080_8A08, &hierarchy, 16);
    let mut bind = [0; 32];
    floats(&mut bind, 0, &[0., 0., 0., 1., 0., 0., 0., 1.]);
    array(&mut skeleton, 0x110, 0x8080_9F75, &bind, 32);
    array(&mut skeleton, 0x120, 0x8080_9F75, &bind, 32);
    let skeleton = package.add(RESOURCE, skeleton);
    let mut clip = vec![0; 0x280];
    put(&mut clip, 0x120, &0x6FB7_60FFu32.to_le_bytes());
    put(&mut clip, 0x13C, &61u16.to_le_bytes());
    put(&mut clip, 0x13E, &1u16.to_le_bytes());
    for at in [0xD8, 0xE8, 0xF8] {
        array(&mut clip, at, 0x8080_000A, &0u16.to_le_bytes(), 2);
    }
    put(&mut clip, 0x18, &(0x200i64 - 0x18).to_le_bytes());
    put(&mut clip, 0x1FC, &0x8080_8F73u32.to_le_bytes());
    for at in [0x202, 0x204, 0x206] {
        put(&mut clip, at, &1u16.to_le_bytes());
    }
    put(&mut clip, 0x210, &61u32.to_le_bytes());
    for (at, class, width, values) in [
        (0x218, 0x8080_000F, 4, vec![1.; 61]),
        (0x228, 0x8080_0096, 16, [0., 0., 0., 1.].repeat(61)),
        (
            0x238,
            0x8080_0091,
            16,
            (0..61)
                .flat_map(|f| [f as f32 / 300., 0., 0., 0.])
                .collect(),
        ),
    ] {
        let values: Vec<u8> = values.into_iter().flat_map(f32::to_le_bytes).collect();
        array(&mut clip, at, class, &values, width);
    }
    let clip = package.add(0x8080_8F49, clip);
    let mut bank = vec![0; 0x18];
    array(&mut bank, 8, 0x8080_8F48, &clip.to_le_bytes(), 4);
    let bank = package.add(0x8080_36F6, bank);
    let mut definition = resource(0x8080_344B);
    put(&mut definition, 0x110, &bank.to_le_bytes());
    [skeleton, package.add(RESOURCE, definition)]
}

fn resource(class: u32) -> Vec<u8> {
    let mut bytes = vec![0; 0x280];
    put(&mut bytes, 0x18, &0x68i64.to_le_bytes());
    put(&mut bytes, 0x7C, &class.to_le_bytes());
    bytes
}

#[test]
fn animated_cloth_follows_its_owner_and_exports_native_moving_anchor_frames() {
    let witness: serde_json::Value = serde_json::from_str(include_str!("moving.json")).unwrap();
    let (directory, tag) = fixture_with_animation(include_bytes!("solver.bin"), false, true);
    let model = fixtures::load(directory.path(), tag).unwrap();
    assert!(model.has_animation(), "{:?}", model.animation_notice);
    assert!(model.has_cloth(), "{:?}", model.notices);
    let mut maximum = 0.0f32;
    for (frame, expected) in witness["frames"].as_array().unwrap().iter().enumerate() {
        let actual = model.pose(frame as f32 / 60.).unwrap();
        for (actual, expected) in actual.positions.iter().zip(expected.as_array().unwrap()) {
            for axis in 0..3 {
                maximum =
                    maximum.max((actual[axis] - expected[axis].as_f64().unwrap() as f32).abs());
            }
        }
    }
    assert!(
        maximum < 0.0005,
        "Moving native trajectory differs by {maximum}"
    );
    let temporary = tempfile::tempdir().unwrap();
    let output = crate::test_support::artifacts("fidelity")
        .unwrap_or_else(|| temporary.path().to_owned())
        .join("cloth-moving");
    fs::create_dir_all(&output).unwrap();
    for frame in [0, 30, 60, 120] {
        let seconds = frame as f32 / 60.;
        fs::write(
            output.join(format!("frame-{frame}.glb")),
            export::glb(&model, seconds).unwrap(),
        )
        .unwrap();
        let image = render::styled_image(
            &model,
            render::Camera::default(),
            render::Scene::default(),
            [256, 256],
            seconds,
            render::Style::Solid,
        );
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        fs::write(
            output.join(format!("frame-{frame}.png")),
            export::png(&rgba, 256, 256).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(model.pose(0.).unwrap().positions, model.vertices);
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(
            &json!({"witness":witness,"maximum_position_error":maximum,"gameplay_verified":false}),
        )
        .unwrap(),
    )
    .unwrap();
}
