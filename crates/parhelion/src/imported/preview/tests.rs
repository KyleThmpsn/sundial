//! Real-window timing of the previews that waited longest: imported weapons and the vehicles a
//! Sparrow summons, with a stock weapon for reference. The ways this could fail:
//! - a preview never draws, or draws only after the time limit
//! - the window stops drawing frames while a model loads, compiles its shader or uploads
//! - an imported recipe no longer opens, so the case list silently shrinks
//!
//! The report goes to `SUNDIAL_TEST_ARTIFACTS/preview-timing/timings-<label>.json`, with the
//! label from `PARHELION_PREVIEW_TIMING_LABEL`.
use super::*;
use crate::vehicle::Summon;
use sundial::test_support::preview_timing::{Case, Subject, run};

/// The longest frame the window may take while a preview opens.
const RESPONSIVE_MS: f64 = 100.0;
/// A stock hand cannon, for reference.
const BETTER_DEVILS: u32 = 0x092D_8A05;

#[test]
#[ignore = "opens an OpenGL window and requires SUNDIAL_INSTALL, SUNDIAL_TEST_ARTIFACTS and PARHELION_PREVIEW_TIMING_RECIPES"]
fn imported_and_summoned_previews_draw_without_stalling_the_window() {
    let packages = crate::test_support::install().join("packages");
    let output = crate::test_support::artifact_dir("preview-timing");
    std::fs::create_dir_all(&output).unwrap();
    let recipes = std::env::var("PARHELION_PREVIEW_TIMING_RECIPES")
        .expect("PARHELION_PREVIEW_TIMING_RECIPES lists imported recipe files, separated by ;");
    // The weapon page's own appearance for a stock item: no shader rows leave its default dyes.
    let catalog = crate::test_support::catalog(&crate::test_support::install()).unwrap();
    let stock = catalog
        .shader_preview_appearance(BETTER_DEVILS, &[Vec::new(), Vec::new(), Vec::new()])
        .expect("Better Devils has an appearance");
    drop(catalog);
    let mut cases = vec![Case {
        name: "Better Devils (stock)".into(),
        subject: Subject::Appearance(stock),
    }];
    for path in recipes.split(';').filter(|path| !path.is_empty()) {
        let recipe = crate::WeaponRecipe::load_json(path).unwrap();
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .unwrap_or_else(|| panic!("{path} has no imported model"));
        cases.push(Case {
            name: format!("{} (imported)", recipe.name),
            subject: Subject::Imported(appearance(reference, recipe.kind)),
        });
    }
    for (name, summon) in [("Tank", Summon::Tank), ("Heavy Pike", Summon::HeavyPike)] {
        cases.push(Case {
            name: format!("{name} (summoned)"),
            subject: Subject::Object(summon.entity().unwrap().unwrap()),
        });
    }
    let label = std::env::var("PARHELION_PREVIEW_TIMING_LABEL").unwrap_or_else(|_| "run".into());
    let mut report = run(
        &packages,
        cases,
        2,
        &output.join(format!("captures-{label}")),
    );
    report["label"] = label.clone().into();
    report["version"] = env!("CARGO_PKG_VERSION").into();
    std::fs::write(
        output.join(format!("timings-{label}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let cases = report["cases"].as_array().unwrap();
    for case in cases {
        assert_eq!(case["outcome"], "drawn", "{}", case["name"]);
    }
    let slowest = cases
        .iter()
        .map(|case| {
            (
                case["frames"]["longest_ms"].as_f64().unwrap(),
                &case["name"],
            )
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    assert!(
        slowest.0 <= RESPONSIVE_MS,
        "{} held the window for {:.0} ms",
        slowest.1,
        slowest.0
    );
}
