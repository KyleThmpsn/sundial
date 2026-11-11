//! Selector input through persisted D1 talents and independent native package readback.
//! The configured corpus must cover attachments, damage with stats, alternatives and no talent.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::recipe::RecipeDamageType;
use crate::test_support::driver::label;
use anyhow::{Context, Result, ensure};
use parhelion_import::tiger::{payload::Payload, reader::Reader};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 500.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                let donor = app.current_donor();
                app.draw_weapon_profile(ui, donor.as_ref());
            });
        },
    );
    capture::record(&output);
    output
}

fn select(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    element: &str,
) -> Result<egui::FullOutput> {
    frame(ctx, app, vec![]);
    let output = frame(ctx, app, vec![]);
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .context("Profile accessibility tree")?
        .nodes;
    let heading = label(&output, "Damage Type").context("Damage Type field")?;
    let (combo, _) = nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::ComboBox)
        .filter_map(|(id, node)| {
            let bounds = node.bounds()?;
            (bounds.y0 >= f64::from(heading.top())).then_some((
                id,
                (bounds.x0 - f64::from(heading.left())).abs()
                    + (bounds.y0 - f64::from(heading.bottom())).abs(),
            ))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .with_context(|| format!("Damage Type combo box: {}", text(&output)))?;
    let click = |target| {
        egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: target,
            data: None,
        })
    };
    frame(ctx, app, vec![click(*combo)]);
    frame(ctx, app, vec![]);
    let output = frame(ctx, app, vec![]);
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .context("Damage menu accessibility tree")?
        .nodes;
    let (option, _) = nodes
        .iter()
        .find(|(_, node)| {
            !matches!(
                node.role(),
                egui::accesskit::Role::Label | egui::accesskit::Role::ComboBox
            ) && node.label().or_else(|| node.value()) == Some(element)
        })
        .with_context(|| {
            format!(
                "Available {element} option on {}: {}",
                app.recipe.name,
                text(&output)
            )
        })?;
    let output = frame(ctx, app, vec![click(*option)]);
    let selected = serde_json::to_value(app.recipe.overrides.modern_damage_type)?;
    ensure!(
        selected == element.to_ascii_lowercase(),
        "Click did not select {element} on {}: {}",
        app.recipe.name,
        text(&output)
    );
    // Accessibility activation has no pointer click to dismiss the popup.
    Ok(frame(
        ctx,
        app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ))
}

/// Exclude only the element setter. Attachments, their patches and equipped stats are independent
/// source contributions whose preservation is observable across save and reload.
fn contributions(recipe: &WeaponRecipe) -> Vec<String> {
    let mut result = Vec::new();
    for variant in &recipe.overrides.socket_plug_variants {
        if !variant.investment_stats.is_empty() {
            result.push(serde_json::to_string(&variant.investment_stats).unwrap());
        }
        for perk in &variant.sandbox_perks {
            let Some(program) = &perk.program else {
                continue;
            };
            let mut value = serde_json::to_value(program).unwrap();
            let actions = value["actions"].as_array_mut().unwrap();
            actions.retain(|action| action["operation"] != "set_damage_type");
            if actions.is_empty() {
                continue;
            }
            value.as_object_mut().unwrap().remove("name");
            result.push(serde_json::to_string(&value).unwrap());
        }
    }
    result.sort();
    result
}

fn table(reader: &mut Reader, slot: usize) -> Result<Payload> {
    let tag = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Investment globals")?
        .hash
        .0;
    let globals = reader.tag(tag, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    Ok((*reader.tag(root.u32(8 + slot * 16)?, None)?).clone())
}

#[test]
#[ignore = "Requires PARHELION_D1_DAMAGE_PLAN, SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn d1_damage_selector_reloads_and_stages_matching_talents() -> Result<()> {
    let plan_path = PathBuf::from(
        std::env::var_os("PARHELION_D1_DAMAGE_PLAN").context("Set PARHELION_D1_DAMAGE_PLAN")?,
    );
    let plan_bytes = fs::read(&plan_path)?;
    let plan: Vec<Value> = serde_json::from_slice(&plan_bytes)?;
    let required = [
        "label_attachment",
        "damage_attachment",
        "damage_stats",
        "alternatives",
        "no_talent",
    ];
    for shape in required {
        ensure!(
            plan.iter().any(|case| case["shape"] == shape),
            "Required {shape} case is missing"
        );
    }
    let output = crate::test_support::artifact_dir("d1-damage");
    ensure!(!output.exists(), "Use fresh D1 damage artifacts");
    fs::create_dir_all(&output)?;
    fs::write(output.join("plan.json"), &plan_bytes)?;
    let packages = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(packages.parent().context("Native root")?)
        .map_err(anyhow::Error::msg)?;
    let donors = catalog.weapon_donors();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        catalog: Some(catalog),
        donor_summaries: donors,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.all_styles_mut(|style| style.animation_time = 0.);
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let mut recipes = Vec::new();
    let mut receipts = Vec::new();
    for (index, case) in plan.iter().enumerate() {
        let path = Path::new(case["recipe"].as_str().context("Configured recipe")?);
        let bytes = fs::read(path)?;
        let source: WeaponRecipe = serde_json::from_slice(&bytes)?;
        let damage_choices = source
            .overrides
            .socket_plug_variants
            .iter()
            .filter(|variant| {
                variant.name.as_deref().is_some_and(|name| {
                    [
                        "Kinetic Damage",
                        "Arc Damage",
                        "Solar Damage",
                        "Void Damage",
                    ]
                    .contains(&name)
                })
            })
            .collect::<Vec<_>>();
        let has_setter = |variant: &&crate::recipe::WeaponSocketPlugVariantRecipe| {
            variant.sandbox_perks.iter()
            .filter_map(|perk| perk.program.as_ref()).flat_map(|program| &program.actions)
            .any(|action| matches!(action, sundial::package_authoring::sandbox_perk::program::Action::SetDamageType { .. }))
        };
        let has_attachment = |variant: &&crate::recipe::WeaponSocketPlugVariantRecipe| {
            variant
                .sandbox_perks
                .iter()
                .filter_map(|perk| perk.program.as_ref())
                .flat_map(|program| &program.actions)
                .any(|action| {
                    serde_json::to_value(action).is_ok_and(|action| action["operation"] == "attach")
                })
        };
        let witnessed = match case["shape"].as_str() {
            Some("label_attachment") => damage_choices
                .iter()
                .any(|v| !has_setter(v) && has_attachment(v)),
            Some("damage_attachment") => damage_choices
                .iter()
                .any(|v| has_setter(v) && has_attachment(v)),
            Some("damage_stats") => damage_choices
                .iter()
                .any(|v| has_setter(v) && !v.investment_stats.is_empty()),
            Some("alternatives") => damage_choices.iter().filter(|v| has_setter(v)).count() > 1,
            Some("no_talent") => damage_choices.is_empty(),
            _ => false,
        };
        ensure!(
            witnessed,
            "Configured input does not witness its promised source shape"
        );
        let original = contributions(&source);
        let (element, expected, mode, marker) =
            match case["shape"].as_str().context("Case shape")? {
                "label_attachment" => ("Solar", RecipeDamageType::Solar, 1, Some(2)),
                "damage_attachment" => ("Void", RecipeDamageType::Void, 3, Some(3)),
                "damage_stats" => ("Arc", RecipeDamageType::Arc, 2, Some(1)),
                "alternatives" => ("Solar", RecipeDamageType::Solar, 1, Some(2)),
                "no_talent" => ("Kinetic", RecipeDamageType::Kinetic, 0, None),
                _ => anyhow::bail!("Unknown source shape"),
            };
        let mut followed_base = source.clone();
        followed_base.overrides.modern_damage_type = None;
        app.recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&followed_base)?)?;
        ensure!(
            app.recipe.overrides.modern_damage_type.is_none(),
            "Loading a legacy recipe introduced a damage override"
        );
        // Exercise repeated changes, including choosing the base donor's own element.
        // The mounted elemental bases can keep native type-68 sockets, which cannot carry
        // Kinetic. The configured Kinetic base witnesses that fourth mode separately.
        for selected in ["Solar", "Void", "Arc", element] {
            select(&ctx, &mut app, selected)?;
        }
        let captured = frame(&ctx, &mut app, vec![]);
        capture::write(&ctx, &captured, &format!("d1-damage-{index}"));
        ensure!(
            app.recipe.overrides.modern_damage_type == Some(expected),
            "Selector did not save {element}"
        );
        let saved = output.join(format!("case-{index}.parhelion.json"));
        fs::write(&saved, app.recipe.to_json_pretty()?)?;
        let reloaded = WeaponRecipe::load_json(&saved)?;
        ensure!(
            contributions(&reloaded) == original,
            "Element edit changed source companion behavior or stats"
        );
        let talent = reloaded
            .overrides
            .socket_plug_variants
            .iter()
            .find(|v| {
                v.choice_index == 0 && v.name.as_deref() == Some(&format!("{element} Damage"))
            })
            .context("Matching default D1 damage talent")?;
        ensure!(
            talent.icon.is_some(),
            "Matching D1 element artwork is absent"
        );
        if case["shape"] == "alternatives" {
            let count = source
                .overrides
                .socket_plug_variants
                .iter()
                .filter(|v| v.socket_index == talent.socket_index)
                .count();
            ensure!(
                reloaded
                    .overrides
                    .socket_plug_variants
                    .iter()
                    .filter(|v| v.socket_index == talent.socket_index)
                    .count()
                    >= count,
                "Source damage alternatives disappeared"
            );
        }
        receipts.push(json!({"source":path,"source_sha256":hex::encode(Sha256::digest(&bytes)),"saved":saved,
            "item":reloaded.identity.item_hash.parse_u32()?,"socket":talent.socket_index,"name":format!("{element} Damage"),
            "mode":mode,"marker":marker,"shape":case["shape"],"donor":source.donor.item_hash.parse_u32()?,
            "templates":source.overrides.socket_plug_variants.iter().map(|v|v.source_plug_hash.parse_u32()).collect::<std::result::Result<Vec<_>,_>>()?}));
        recipes.push(reloaded);
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes,
    })
    .map_err(anyhow::Error::msg)?;
    let built = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {})
        .map_err(anyhow::Error::msg)?;
    let view = output.join("view");
    fs::create_dir_all(view.join("packages"))?;
    fs::create_dir_all(view.join("bin/x64"))?;
    let native_root = packages.parent().context("Native root")?;
    fs::copy(native_root.join("destiny2.exe"), view.join("destiny2.exe"))?;
    fs::copy(
        native_root.join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for entry in fs::read_dir(&packages)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), view.join("packages").join(entry.file_name()))?;
        }
    }
    let mut package_hashes = Vec::new();
    for artifact in &built.artifacts {
        let bytes = fs::read(built.run_directory.join(&artifact.file_name))?;
        let target = view.join("packages").join(&artifact.file_name);
        ensure!(!target.exists(), "Staging would overwrite a stock package");
        fs::write(target, &bytes)?;
        package_hashes
            .push(json!({"file":artifact.file_name,"sha256":hex::encode(Sha256::digest(&bytes))}));
    }
    let staged_catalog = InvestmentCatalog::load_with_cache_path(
        &view,
        &output.join("staged-catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let mut reader = Reader::new(&view.join("packages"), &output.join("readback"), false)?;
    let items = table(&mut reader, 48)?;
    let item_rows = items.array(8, 24, Some(0x80807BE8))?;
    let globals_tag = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals_tag, None)?;
    let finished = reader.tag(globals.u32(16 + 71 * 16)?, Some(0x80805C97))?;
    let finished_rows = finished.array(8, 24, Some(0x80805C9D))?;
    let runtime = reader.tag(0x80EC3F60, None)?;
    let runtime_rows = runtime.array(8, 8, None)?;
    let mut stock = Reader::new(&packages, &output.join("stock-readback"), false)?;
    let stock_items = table(&mut stock, 48)?;
    let stock_rows = stock_items.array(8, 24, Some(0x80807BE8))?;
    let mut unchanged = BTreeMap::new();
    for receipt in &receipts {
        for hash in std::iter::once(&receipt["donor"])
            .chain(receipt["templates"].as_array().context("Stock templates")?)
        {
            let hash = hash.as_u64().context("Stock item identity")? as u32;
            let row = *stock_rows
                .iter()
                .find(|&&r| stock_items.u32(r).ok() == Some(hash))
                .context("Stock item")?;
            let tag = stock_items.u32(row + 16)?;
            let bytes = stock.tag(tag, Some(0x80807BEA))?;
            ensure!(
                reader.tag(tag, Some(0x80807BEA))?.0 == bytes.0,
                "Stock donor or perk template changed"
            );
            unchanged.insert(format!("{tag:08X}"), hex::encode(Sha256::digest(&bytes.0)));
        }
    }
    for receipt in &mut receipts {
        let hash = receipt["item"].as_u64().context("Item")? as u32;
        let row = *item_rows
            .iter()
            .find(|&&r| items.u32(r).ok() == Some(hash))
            .context("Staged item")?;
        let definition = reader.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        let sockets = definition.array(definition.pointer(0x68)?, 80, Some(0x808077C4))?;
        let markers = definition
            .array(definition.pointer(0x70)? + 16, 24, Some(0x808077BC))?
            .into_iter()
            .filter_map(|at| match definition.u16(at).ok()? {
                83 | 449 => Some(1),
                84 | 450 => Some(2),
                85 | 451 => Some(3),
                _ => None,
            })
            .chain(sockets.iter().filter_map(|&at| {
                (definition.u16(at).ok()? == 68)
                    .then(|| match definition.u16(at + 2).ok()? {
                        1505 => Some(1),
                        1506 => Some(2),
                        1507 => Some(3),
                        _ => None,
                    })
                    .flatten()
            }))
            .collect::<Vec<_>>();
        let expected_markers = receipt["marker"]
            .as_u64()
            .map(|n| n as u8)
            .into_iter()
            .collect::<Vec<_>>();
        ensure!(
            markers == expected_markers,
            "Native carrier differs from selected damage talent"
        );
        let socket = sockets[receipt["socket"].as_u64().context("Socket")? as usize];
        let plug_index = usize::from(definition.u16(socket + 2)?);
        let plug_row = item_rows[plug_index];
        let plug_hash = items.u32(plug_row)?;
        ensure!(
            staged_catalog.item_display_name(plug_hash) == receipt["name"].as_str(),
            "Staged talent label differs"
        );
        let plug = reader.tag(items.u32(plug_row + 16)?, Some(0x80807BEA))?;
        let mut modes = Vec::new();
        for perk in plug.array(plug.pointer(0x70)? + 16, 24, Some(0x808077BC))? {
            let index = usize::from(plug.u16(perk)?);
            let key = finished.u32(*finished_rows.get(index).context("Finished perk")? + 4)?;
            let assignment = *runtime_rows
                .iter()
                .find(|&&r| runtime.u32(r).ok() == Some(key))
                .context("Private runtime assignment")?;
            let action = reader.tag(runtime.u32(assignment + 4)?, None)?;
            let decoded = sundial::package_authoring::sandbox_perk::action::decode(&action.0)
                .map_err(anyhow::Error::msg)?;
            for effect in decoded.effects().filter(|effect| effect.kind == 6) {
                modes.push(action.u8(effect.offset + 2)?);
            }
        }
        ensure!(
            modes == vec![receipt["mode"].as_u64().context("Expected mode")? as u8],
            "Emitted D1 setter differs: {modes:?}"
        );
        receipt["private_plug"] = json!(plug_hash);
        receipt["observed_modes"] = json!(modes);
        receipt["observed_markers"] = json!(markers);
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":std::env::var("SUNDIAL_TEST_REVISION")?,"binary_sha256":hex::encode(Sha256::digest(fs::read(std::env::current_exe()?)?)),
            "native_packages":packages,"plan_sha256":hex::encode(Sha256::digest(&plan_bytes)),"packages":package_hashes,"cases":receipts,"unchanged_stock_definitions":unchanged,
            "gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
