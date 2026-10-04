//! Opt-in format diagnostic. Skeletons are synthetic, so this does not certify native rigs.
use super::*;
use serde_json::json;

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_FIDELITY_OUTPUT"]
fn native_clip_codec_corpus() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let output = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT").expect("artifact directory");
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let mut receipt = Vec::new();
    let mut errors = Vec::new();
    for (tag, _) in manager.get_all_by_reference(0x8080_8F49) {
        let bytes = checked(&manager, tag.0, 0x8080_8F49).unwrap();
        let bones = u16_at(&bytes, 0x13E).unwrap() as usize;
        let mut skeleton = vec![0; 0xB0];
        let hierarchy: Vec<_> = (0..bones)
            .flat_map(|i| [i as i32, -1, -1, -1])
            .flat_map(i32::to_le_bytes)
            .collect();
        tests::append_array(&mut skeleton, 0x80, 0x8080_8A08, bones, &hierarchy);
        let inverse: Vec<_> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .into_iter()
            .cycle()
            .take(bones * 8)
            .flat_map(f32::to_le_bytes)
            .collect();
        tests::append_array(&mut skeleton, 0xA0, 0x8080_9F75, bones, &inverse);
        let dynamic = if i64_at(&bytes, 0x18).unwrap() == 0 {
            0
        } else {
            u32_at(&bytes, pointer(&bytes, 0x18).unwrap() - 4).unwrap()
        };
        let result = match decode(tag.0, &bytes, &skeleton, 0) {
            Ok(animation) => {
                assert!(animation.poses.iter().flatten().all(|p| {
                    p.rotation
                        .iter()
                        .chain(&p.translation)
                        .chain(std::iter::once(&p.scale))
                        .all(|v| v.is_finite())
                }));
                json!({"tag":format!("{:08X}",tag.0),"codec":format!("{dynamic:08X}"),"frames":animation.frames,"bones":bones,"decoded":true})
            }
            Err(error) => {
                errors.push(format!("{tag}: {error}"));
                json!({"tag":format!("{:08X}",tag.0),"codec":format!("{dynamic:08X}"),"decoded":false,"error":error})
            }
        };
        receipt.push(result);
    }
    let output = Path::new(&output);
    std::fs::create_dir_all(output).unwrap();
    std::fs::write(output.join("codec-corpus.json"),serde_json::to_vec_pretty(&json!({
        "scope":"Native clip codecs against synthetic identity skeletons",
        "native_rig_verified":false,"clips":receipt.len(),"failures":errors.len(),"results":receipt
    })).unwrap()).unwrap();
    assert!(!receipt.is_empty());
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
