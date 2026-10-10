//! The technical overrides a recipe uses, as the report lists them.
use super::*;

pub(super) fn technical_recipe_features(recipe: &WeaponRecipe) -> Vec<String> {
    let overrides = &recipe.overrides;
    let mut features = Vec::new();
    for (active, label) in [
        (
            !recipe.runtime_component_donors.is_empty(),
            "Advanced: runtime component donors",
        ),
        (
            !overrides.runtime_values.is_empty(),
            "Advanced: edited runtime values",
        ),
        (
            overrides.base_sandbox_perks.is_some(),
            "Advanced: base weapon perks",
        ),
        (overrides.trait_indices.is_some(), "Advanced: trait indices"),
        (
            !overrides.runtime_resource_patches.is_empty(),
            "Advanced: runtime resource patches",
        ),
        (
            !overrides.additional_behaviors.is_empty(),
            "Additional behavior",
        ),
        (
            !overrides.raw_payload_patches.is_empty(),
            "Advanced: raw payload patches",
        ),
        (
            overrides.max_stack_size.is_some(),
            "Advanced: maximum stack size",
        ),
        (
            overrides.socket_entry_list_index.is_some(),
            "Advanced: socket entry list",
        ),
        (
            overrides.plug_category_hash.is_some(),
            "Advanced: plug category",
        ),
        (overrides.roll_set_index.is_some(), "Advanced: roll set"),
        (
            overrides.linked_plug_index.is_some(),
            "Advanced: linked plug",
        ),
        (
            overrides.power_cap_groups.is_some(),
            "Advanced: Power cap groups",
        ),
        (
            overrides.art_arrangements.is_some(),
            "Appearance: art arrangement rows",
        ),
        (
            overrides.render_dye_rows.is_some(),
            "Appearance: render dye rows",
        ),
        (
            overrides.subclass_abilities.is_some(),
            "Abilities: authored or from other subclasses",
        ),
    ] {
        if active {
            features.push(label.to_owned());
        }
    }
    for (index, column) in overrides
        .socket_columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| column.as_ref().map(|column| (index, column)))
    {
        // Socket role is already editable in the ordinary Weapon tab.
        let fields = [
            (!column.choice_weight_bits.is_empty(), "choice weights"),
            (
                !column.choice_conditions.is_empty(),
                "choice availability conditions",
            ),
            (
                column.reusable_plug_set_index.is_some(),
                "reusable plug set",
            ),
            (
                column.randomized_plug_set_index.is_some(),
                "randomized plug set",
            ),
            (
                !column.randomized_selection_program.is_empty(),
                "random choice count program",
            ),
        ]
        .into_iter()
        .filter_map(|(active, label)| active.then_some(label))
        .collect::<Vec<_>>();
        if !fields.is_empty() {
            features.push(format!("Socket {}: {}. Edit under Weapon → Perks & Sockets → More Options → Show Socket Details.", index + 1, fields.join(", ")));
        }
    }
    features
}
