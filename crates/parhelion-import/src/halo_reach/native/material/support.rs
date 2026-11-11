use super::*;

/// Reject missing resources instead of producing the appearance of another permutation.
/// Runtime inputs that have no native binding remain explicit conversion limits.
pub(super) fn validate(
    material: &Material,
    samples: &BTreeMap<usize, String>,
    notes: &mut Vec<String>,
) -> Result<()> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let require = |slots: &[usize], feature: &str| -> Result<()> {
        for slot in slots {
            ensure!(
                samples.contains_key(slot),
                "{feature} requires missing texture role {slot}"
            );
        }
        Ok(())
    };
    if material.tag.as_ref().is_some_and(|t| t.group == "rmsh") {
        match option("albedo") {
            Some("two_detail" | "detail_blend" | "two_detail_black_point") => {
                require(&[0, 5, DETAIL2_SLOT], "Layered albedo")?
            }
            Some("three_detail_blend") => {
                require(&[0, 5, DETAIL2_SLOT, DETAIL3_SLOT], "Three-detail albedo")?
            }
            Some("two_detail_overlay") => {
                require(&[0, 5, DETAIL2_SLOT, DETAIL_OVERLAY_SLOT], "Overlay albedo")?
            }
            Some("color_mask") => require(&[0, 5, COLOR_MASK_SLOT], "Color-mask albedo")?,
            None
            | Some(
                "default"
                | "constant_color"
                | "two_change_color"
                | "four_change_color"
                | "four_change_color_applying_to_specular"
                | "simple",
            ) => (),
            other => anyhow::bail!("Unsupported surface albedo mode {other:?}"),
        }
        match option("bump_mapping") {
            Some("detail" | "detail_wrinkle") => require(&[1, BUMP_DETAIL_SLOT], "Detail normal")?,
            Some("detail_blend") => require(
                &[0, 1, BUMP_DETAIL_SLOT, BUMP_DETAIL2_SLOT],
                "Blended detail normal",
            )?,
            Some("three_detail_blend") => require(
                &[0, 1, BUMP_DETAIL_SLOT, BUMP_DETAIL2_SLOT, BUMP_DETAIL3_SLOT],
                "Three-detail normal",
            )?,
            None | Some("off" | "standard" | "standard_wrinkle") => (),
            other => anyhow::bail!("Unsupported surface bump mode {other:?}"),
        }
        if matches!(
            option("bump_mapping"),
            Some("standard_wrinkle" | "detail_wrinkle")
        ) {
            require(
                &[1, normal::WRINKLE, normal::MASK_A, normal::MASK_B],
                "Wrinkle normal",
            )?;
            notes.push("Wrinkle normals use the authored masks and material weights. Facial-animation wrinkle inputs need a native object binding before they can animate.".into());
        }
    }
    emission(material, samples, notes, true)
}

pub(super) fn emission(
    material: &Material,
    samples: &BTreeMap<usize, String>,
    notes: &mut Vec<String>,
    scene_depth: bool,
) -> Result<()> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    let require = |slots: &[usize], feature: &str| -> Result<()> {
        for slot in slots {
            ensure!(
                samples.contains_key(slot),
                "{feature} requires missing texture role {slot}"
            );
        }
        Ok(())
    };
    match option("self_illumination") {
        None | Some("off" | "none" | "from_diffuse" | "from_albedo") => (),
        Some("window_room") => notes.push(
            "MCC's DirectX 11 window-room mode returns zero emission. Its room-texture path is limited to DirectX 9 in the source shader.".into(),
        ),
        Some(
            "simple"
            | "simple_with_alpha_mask"
            | "3_channel_self_illum"
            | "meter"
            | "multilayer_additive"
            | "scope_blur"
            | "change_color"
            | "times_diffuse"
            | "self_illum_times_diffuse",
        ) => require(&[4], "Self illumination")?,
        Some("illum_detail" | "change_color_detail") => {
            require(&[4, ILLUM_DETAIL_SLOT], "Detail self illumination")?
        }
        Some("plasma") => require(&[3, 6, 7], "Plasma self illumination")?,
        Some("palettized_plasma" | "palettized_plasma_change_color") => {
            require(&[3, 6, 7, 8], "Palettized plasma")?
        }
        Some("palettized_depth_fade") => require(&[3, 6, 8], "Palettized depth fade")?,
        other => anyhow::bail!("Unsupported surface self-illumination mode {other:?}"),
    }
    if matches!(
        option("self_illumination"),
        Some("palettized_plasma" | "palettized_plasma_change_color" | "palettized_depth_fade")
    ) && material.alpha_mode() == "BLEND"
    {
        notes.push(if scene_depth {
            "Palettized emission applies its scene-depth intersection fade through the native transparent depth scope. A target-size mismatch keeps full visibility. Matching dimensions alone do not prove resource identity, so intersection behavior still requires a live draw capture."
        } else {
            "Sky palettized emission has no scene-depth intersection binding and retains full depth visibility."
        }.into());
    }
    Ok(())
}
