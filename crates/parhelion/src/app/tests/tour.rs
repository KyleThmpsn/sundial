//! Every workbench page rendered as the window draws it, for design review. Each page is drawn
//! twice, on a wide and a narrow window, for a plain weapon and for one with every part row,
//! component and placement choice set, and written as captures when `SUNDIAL_TEST_ARTIFACTS`
//! is set. The canvas is tall, so nothing a page holds hides below a scroll.
use super::*;
use crate::recipe::{ComponentSpliceRecipe, MarkerOffsetRecipe, WeaponDonorReference};
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY,
};

const JADE_RABBIT: u32 = 0xE529_6126;
const MACHINA_DEI_4: u32 = 0x09A0_DE64;
const SWEET_BUSINESS: u32 = 0x5038_4F32;

fn reference(item_hash: u32, name: &str) -> WeaponDonorReference {
    WeaponDonorReference {
        item_hash: item_hash.into(),
        expected_name: Some(name.to_owned()),
    }
}

/// Draws the whole window until background reads settle, and returns the last frame.
fn settle(
    app: &mut PackageAuthoringApp,
    ctx: &egui::Context,
    size: egui::Vec2,
) -> egui::FullOutput {
    let started = std::time::Instant::now();
    let mut quiet = 0;
    loop {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| app.update_ui(ui),
        );
        custom_perks::workbench::tests::capture::record(&output);
        let rendered = text(&output);
        // Gameplay's Barrel Settings and projectile cards read in the background as "Loading…".
        let busy = rendered.contains("Reading\u{2026}")
            || rendered.contains("Loading\u{2026}")
            || rendered.contains("Loading Model")
            || rendered.contains("Checking the rig")
            || rendered.contains("Reading runtime components");
        quiet = if busy { 0 } else { quiet + 1 };
        // A few quiet frames let the model's software image land after the page settles.
        if quiet >= 40 || started.elapsed() > std::time::Duration::from_secs(240) {
            return output;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL; package-backed design review captures"]
fn real_workbench_tour() {
    let packages = crate::test_support::install().join("packages");
    let plain =
        WeaponRecipe::new_weapon_for_donor("parhelion.tour-plain", JADE_RABBIT, "The Jade Rabbit")
            .unwrap();
    let mut configured = plain.clone();
    configured.namespace = "parhelion.tour-configured".to_owned();
    configured.name = "Fate of All Fools".to_owned();
    configured.set_presentation_donor(Some(WeaponDonorReference {
        item_hash: MACHINA_DEI_4.into(),
        expected_name: Some("Machina Dei 4".to_owned()),
    }));
    configured.overrides.animation_donor = Some(reference(JADE_RABBIT, "The Jade Rabbit"));
    configured.overrides.component_splices = vec![
        ComponentSpliceRecipe {
            binding_hash: WEAPON_BARREL_COMPONENT_KEY.into(),
            donor: reference(MACHINA_DEI_4, "Machina Dei 4"),
        },
        ComponentSpliceRecipe {
            binding_hash: WEAPON_MAGAZINE_COMPONENT_KEY.into(),
            donor: reference(SWEET_BUSINESS, "Sweet Business"),
        },
    ];
    configured.overrides.marker_offsets = vec![MarkerOffsetRecipe {
        marker: sundial::package_authoring::fnv1_name_hash("primary_fire").into(),
        offset_um: [10_000, 0, 0],
    }];
    configured.overrides.held_offset_um = [20_000, 0, -5_000];
    for (state, recipe) in [("plain", plain), ("configured", configured)] {
        for size in [egui::vec2(1600.0, 2400.0), egui::vec2(1000.0, 2400.0)] {
            let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
            let mut app = PackageAuthoringApp {
                donor_summaries: catalog.weapon_donors(),
                sandbox_perk_choices: catalog.weapon_sandbox_perk_choices_from(|_| true),
                catalog: Some(catalog),
                // The catalog is the test's own copy, so the app must not start its own load.
                catalog_load_requested: true,
                packages: packages.clone(),
                recipe: recipe.clone(),
                // The component rows and the other technical controls show only with this on.
                show_experimental_options: true,
                ..Default::default()
            };
            app.library_state.refresh_donors(&app.donor_summaries);
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let slug = |label: &str| label.to_lowercase().replace([' ', '&'], "-");
            for page in WorkbenchPage::ALL {
                app.workbench_page = page;
                let output = settle(&mut app, &ctx, size);
                let name = format!("tour-{state}-{}-{}", slug(page.label()), size.x as u32);
                custom_perks::workbench::tests::capture::write(&ctx, &output, &name);
            }
        }
    }
}
