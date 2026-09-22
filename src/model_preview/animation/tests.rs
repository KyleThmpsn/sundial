use super::*;

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn append_array(bytes: &mut Vec<u8>, offset: usize, class: u32, count: usize, rows: &[u8]) {
    let start = bytes.len();
    put_u64(bytes, offset, count as u64);
    put_u64(bytes, offset + 8, (start - offset - 8) as u64);
    bytes.extend_from_slice(&(count as u64).to_le_bytes());
    bytes.extend_from_slice(&class.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(rows);
}

#[test]
fn uniform_codec_decodes_static_and_animated_tracks_and_rejects_bad_counts() {
    let mut skeleton = vec![0; 256];
    let hierarchy = [0_i32, -1, 1, -1, 1, 0, -1, -1]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<_>>();
    append_array(&mut skeleton, 0x80, 0x80808A08, 2, &hierarchy);
    let inverse = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        .repeat(2)
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    append_array(&mut skeleton, 0xA0, 0x80809F75, 2, &inverse);
    let mut clip = vec![0; 0x300];
    put_u64(&mut clip, 0x10, 0x200 - 0x10);
    put_u64(&mut clip, 0x18, 0x280 - 0x18);
    clip[0x1FC..0x200].copy_from_slice(&0x80808F6F_u32.to_le_bytes());
    clip[0x27C..0x280].copy_from_slice(&0x80808F71_u32.to_le_bytes());
    clip[0xA4..0xA8].copy_from_slice(&30_u32.to_le_bytes());
    put_u16(&mut clip, 0x13C, 2);
    put_u16(&mut clip, 0x13E, 2);
    for (offset, values) in [
        (0xA8, vec![0_u16, 1]),
        (0xB8, vec![0]),
        (0xC8, vec![0, 1]),
        (0xE8, vec![1]),
        (0xF8, vec![]),
    ] {
        let rows = values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        append_array(&mut clip, offset, 0x8080000A, values.len(), &rows);
    }
    for (offset, value) in [
        (0x200, 3),
        (0x202, 2),
        (0x204, 1),
        (0x206, 2),
        (0x280, 2),
        (0x284, 1),
    ] {
        put_u16(&mut clip, offset, value);
    }
    clip[0x214..0x218].copy_from_slice(&1.0_f32.to_le_bytes());
    clip[0x290..0x294].copy_from_slice(&2_u32.to_le_bytes());
    let fixed = [0_u16, 0, 32768, 32768, 32768, 65535, 0, 0, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    append_array(&mut clip, 0x238, 0x8080000A, 12, &fixed);
    let animated = [32768_u16, 32768, 32768, 65535, 32768, 32768, 55938, 55938]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    append_array(&mut clip, 0x298, 0x8080000A, 8, &animated);
    for (offset, value) in [(0x2A8, 2.0_f32), (0x2B8, -1.0_f32)] {
        append_array(
            &mut clip,
            offset,
            0x8080000F,
            4,
            &[value; 4]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    let len = clip.len();
    put_u64(&mut clip, 0, len as u64);
    let animation = decode(0, &clip, &skeleton, 0).unwrap();
    let model = Model {
        vertices: vec![[1.0, 0.0, 0.0]],
        weights: vec![Some(Weights {
            values: [255, 0, 0, 0],
            bones: [1, 255, 255, 255],
        })],
        ..Default::default()
    };
    let vertex = animation.vertices(&model, animation.duration())[0];
    assert!(vertex[0].abs() < 0.001 && (vertex[1] - 1.0).abs() < 0.001);
    put_u16(&mut clip, 0x284, 2);
    assert!(decode(0, &clip, &skeleton, 0).is_err());
    assert!(decode(0, &clip[..128], &skeleton, 0).is_err());
}

#[test]
fn skinning_uses_parent_motion_and_inverse_bind_pose() {
    let mut parent = Transform::identity();
    parent.translation = [3.0, 0.0, 0.0];
    let mut child = Transform::identity();
    child.translation = [0.0, 2.0, 0.0];
    let mut inverse = Transform::identity();
    inverse.translation = [-3.0, -2.0, 0.0];
    let mut moved = parent;
    moved.translation[0] += 2.0;
    let animation = Animation {
        tag: 0,
        frames: 2,
        fps: 1.0,
        parents: vec![None, Some(0)],
        inverse: vec![Transform::identity(), inverse],
        poses: vec![vec![parent, child], vec![moved, child]],
    };
    let model = Model {
        vertices: vec![[4.0, 2.0, 0.0]],
        weights: vec![Some(Weights {
            values: [255, 0, 0, 0],
            bones: [1, 255, 255, 255],
        })],
        ..Default::default()
    };
    assert_eq!(animation.vertices(&model, 0.0), model.vertices);
    assert_eq!(animation.vertices(&model, 0.5), vec![[5.0, 2.0, 0.0]]);
    assert_eq!(animation.vertices(&model, 1.0), vec![[6.0, 2.0, 0.0]]);
}

#[test]
fn quaternion_interpolation_takes_the_short_arc() {
    let first = Transform::identity();
    let mut second = first;
    second.rotation = [0.0, 0.0, 0.0, -1.0];
    assert_eq!(
        first.lerp(second, 0.5).point([1.0, 2.0, 3.0]),
        [1.0, 2.0, 3.0]
    );
    second.rotation = [f32::NAN, 0.0, 0.0, 1.0];
    assert!(second.normalize().is_err());
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
fn chicken_idle_decodes_and_deforms_without_changing_topology() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let model = load_model(Path::new(&packages));
    let animation = model.animation.as_ref().expect("native chicken idle");
    assert_eq!(animation.tag, 0x80BC90D2);
    assert_eq!((animation.frames, animation.fps), (121, 30.0));
    assert_eq!(animation.parents.len(), 31);
    assert_eq!(animation.duration(), 4.0);
    let first = animation.vertices(&model, 0.0);
    let middle = animation.vertices(&model, 2.0);
    assert_eq!(first.len(), model.vertices.len());
    let movement = first
        .iter()
        .zip(&middle)
        .map(|(a, b)| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt())
        .fold(0.0, f32::max);
    assert!(movement > 0.01 && movement < 0.5, "movement {movement}");
    for frame in 0..animation.frames {
        let vertices = animation.vertices(&model, frame as f32 / animation.fps);
        assert!(
            vertices
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() < 2.0)
        );
    }
    if let Some(output) = std::env::var_os("SUNDIAL_ANIMATION_OUTPUT") {
        let path = Path::new(&output);
        std::fs::create_dir_all(path).unwrap();
        for frame in 0..40 {
            let image = render::animated_image(
                &model,
                render::Camera::default(),
                [400, 400],
                frame as f32 / 10.0,
            );
            let mut bytes = b"P6\n400 400\n255\n".to_vec();
            bytes.extend(
                image
                    .pixels
                    .iter()
                    .flat_map(|color| [color.r(), color.g(), color.b()]),
            );
            std::fs::write(path.join(format!("idle-{frame:02}.ppm")), bytes).unwrap();
        }
    }
    // Exercise failure paths against the same verified native clip.
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let bytes = checked(&manager, animation.tag, 0x80808F49).unwrap();
    let skeleton = checked(&manager, 0x80BC9090, RESOURCE).unwrap();
    for end in [0, 0x138, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(animation.tag, &bytes[..end], &skeleton, 0x250).is_err());
    }
    let mut bad = bytes.clone();
    bad[0x13C..0x13E].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(decode(animation.tag, &bad, &skeleton, 0x250).is_err());
}

fn load_model(packages: &Path) -> Model {
    let model = super::super::load(packages, 0x80BC90E3).unwrap();
    assert!(
        model.animation_notice.is_none(),
        "{:?}",
        model.animation_notice
    );
    model
}
