//! Resolves the same appearance variant used when inheriting the compiled HUD icon.
use tiger_pkg::PackageManager;

pub(super) fn load(
    manager: &PackageManager,
    pattern_index: u16,
) -> Result<Option<egui::ColorImage>, String> {
    let source =
        sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_at_pattern_index_with_manager(
            manager, pattern_index,
        )?;
    let key =
        super::runtime::inherited_key(manager, &source.payload, source.weapon_content_group_hash)
            .map_err(|error| error.to_string())?;
    super::runtime::icon_layer(manager, key)
        .map_err(|error| error.to_string())?
        .map(|layer| load_layer(manager, layer))
        .transpose()
}

fn load_layer(
    manager: &PackageManager,
    tag: tiger_pkg::TagHash,
) -> Result<egui::ColorImage, String> {
    use crate::tag_payload::{array_at, read_u32};
    let read = || -> crate::AuthoringResult<_> {
        let entry = manager
            .get_entry(tag)
            .ok_or_else(|| crate::error::invalid("HUD icon layer missing"))?;
        if entry.file_type != 8 || entry.file_subtype != 0 || entry.reference != 0x80804A69 {
            return Err(crate::error::invalid("Unsupported HUD icon layer type"));
        }
        let layer = manager
            .read_tag(tag)
            .map_err(|error| crate::error::invalid(error.to_string()))?;
        if layer.len() != entry.file_size as usize || read_u32(&layer, 0)? as usize != layer.len() {
            return Err(crate::error::invalid("HUD icon layer size is invalid"));
        }
        // HUD layers use distinct lane/texture classes from inventory icons.
        let (count, _, lanes, class) = array_at(&layer, 0x20)?;
        if count != 1 || class != 0x80804A6A {
            return Err(crate::error::invalid("Unsupported HUD icon lane layout"));
        }
        let (count, _, textures, class) = array_at(&layer, lanes)?;
        if count != 1 || class != 0x80804A6E || textures.checked_add(4) != Some(layer.len()) {
            return Err(crate::error::invalid("Unsupported HUD icon texture layout"));
        }
        read_u32(&layer, textures).map(tiger_pkg::TagHash)
    };
    crate::icon_edit::render_texture_preview(manager, read().map_err(|error| error.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PARHELION_HUD_TEST_PACKAGES pointing to Shadowkeep packages"]
    fn native_preview_selects_the_appearance_variant_and_preserves_alpha() {
        let packages =
            std::path::PathBuf::from(std::env::var_os("PARHELION_HUD_TEST_PACKAGES").unwrap());
        let manager =
            sundial::package_authoring::open_shadowkeep_package_manager(&packages).unwrap();
        let catalog =
            sundial::investment::InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {})
                .unwrap();
        let donors = catalog.weapon_donors();
        let pattern = |hash| {
            donors
                .iter()
                .find(|donor| donor.hash == hash)
                .unwrap()
                .weapon_pattern_index
                .unwrap()
        };
        let bond = load(&manager, pattern(0x23DB_942F)).unwrap().unwrap();
        assert_eq!(bond.size, [137, 76]);
        assert!(bond.pixels.iter().any(|pixel| pixel.a() == 0));
        assert!(bond.pixels.iter().any(|pixel| pixel.a() > 0));
        let talon = load(&manager, pattern(0xE079_4C51)).unwrap().unwrap();
        let hook = load(&manager, pattern(0x0222_2CBF)).unwrap().unwrap();
        assert_ne!(
            talon.pixels, hook.pixels,
            "shared sword content owners must select their own silhouette"
        );
    }
}
