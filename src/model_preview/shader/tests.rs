use super::*;

fn dye() -> Surface {
    Surface {
        albedo: [0.8, 0.1, 0.05],
        worn_albedo: [0.1, 0.2, 0.8],
        params: [1.0, 0.0, 0.5, 0.0],
        worn_params: [0.0, 0.0, 0.0, 1.0],
        wear: [0.0, 1.0, 0.0, 1.0],
        roughness: [0.7, 0.0, 0.0, 1.0],
        worn_roughness: [0.2, 0.0, 0.0, 1.0],
        emissive: [0.0, 0.5, 1.0],
        iridescence: -1.0,
    }
}

#[test]
fn mask_preserves_undyed_color_and_metal_when_shader_changes() {
    let base = [0.1, 0.2, 0.3];
    let mask = [255.0, 128.0, 40.0, 16.0];
    let a = evaluate(base, mask, Some([0.0; 4]), &dye());
    let b = evaluate(base, mask, None, &Surface::default());
    assert_eq!(a.albedo, base);
    assert_eq!(a.albedo, b.albedo);
    assert_eq!(a.metal, 0.5);
    assert_eq!(a.metal, b.metal);
    assert_eq!(light(a, [0.0, 0.0, -1.0]), light(b, [0.0, 0.0, -1.0]));
}

#[test]
fn wear_selects_worn_color_metal_and_roughness_at_the_correct_end() {
    let worn = evaluate([0.25; 3], [255.0, 128.0, 40.0, 48.0], None, &dye());
    let intact = evaluate([0.25; 3], [255.0, 128.0, 40.0, 255.0], None, &dye());
    assert_eq!(worn.albedo, dye().worn_albedo);
    assert!(
        intact
            .albedo
            .into_iter()
            .zip(dye().albedo)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
    );
    assert_eq!(worn.metal, 1.0);
    assert_eq!(intact.metal, 0.0);
    assert!((worn.roughness - 0.8).abs() < 1e-6);
    assert!((intact.roughness - 0.3).abs() < 1e-6);
}

#[test]
fn detail_color_and_alpha_change_only_dyeable_materials() {
    let mask = [255.0, 128.0, 40.0, 255.0];
    let mut dye = dye();
    dye.roughness = [0.0, 1.0, 0.0, 1.0];
    let base = evaluate([0.25; 3], mask, None, &dye);
    let detail = evaluate([0.25; 3], mask, Some([80.0, 128.0, 200.0, 255.0]), &dye);
    assert_ne!(base.albedo, detail.albedo);
    assert_ne!(base.roughness, detail.roughness);
    dye.params[0] = 0.0;
    dye.params[2] = 0.0;
    let disabled = evaluate([0.25; 3], mask, Some([80.0, 128.0, 200.0, 255.0]), &dye);
    assert_eq!(base.albedo, disabled.albedo);
    assert_eq!(base.roughness, disabled.roughness);
}

#[test]
fn emission_survives_occlusion_and_finish_changes_the_highlight() {
    let lit = evaluate([0.25; 3], [0.0, 128.0, 255.0, 255.0], None, &dye());
    let mut unlit = evaluate([0.25; 3], [0.0, 128.0, 255.0, 255.0], None, &dye());
    unlit.emission = [0.0; 3];
    let (lit, unlit) = (light(lit, [0.0, 0.0, -1.0]), light(unlit, [0.0, 0.0, -1.0]));
    for i in 0..3 {
        assert!((lit[i] - unlit[i] - dye().emissive[i]).abs() < 1e-5);
    }
    let sample = |roughness, metal| Sample {
        albedo: [0.4; 3],
        roughness,
        metal,
        ao: 1.0,
        emission: [0.0; 3],
    };
    let n = [-0.187, -0.293, -0.937];
    assert!(light(sample(0.1, 1.0), n)[0] > light(sample(0.9, 1.0), n)[0]);
    assert_ne!(light(sample(0.5, 0.0), n), light(sample(0.5, 1.0), n));
}

#[test]
fn gamma_tables_match_the_exact_curves() {
    for i in 0..=255 {
        let value = i as f32 / 255.0;
        assert!((linear(value) - linear_exact(value)).abs() < 2e-4, "{i}");
        assert_eq!(encode(linear_exact(value)), i, "round trip {i}");
    }
}

#[test]
fn native_remap_uses_bias_scale_and_output_bounds() {
    assert_eq!(remap(0.2, [-4.6, 16.33, 0.74, 0.15]), 0.74);
    assert!((remap(0.9, [-4.6, 16.33, 0.74, 0.15]) - 0.89).abs() < 1e-6);
    assert_eq!(remap(0.9, [0.0, 1.0, 0.5, 0.0]), 0.5);
}
