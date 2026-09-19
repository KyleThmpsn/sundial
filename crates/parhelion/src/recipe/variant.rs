//! Converts portable perk definitions through the same validation used by weapons.
use super::*;

impl WeaponSocketPlugVariantRecipe {
    pub(crate) fn validate(&self) -> Result<(), RecipeError> {
        let variant = self.to_compiler(0)?;
        crate::weapon::validate_socket_plug_variant_shapes(&[variant])?;
        Ok(())
    }

    pub(super) fn to_compiler(
        &self,
        variant_index: usize,
    ) -> Result<WeaponSocketPlugVariantOverride, RecipeError> {
        let context = format!("socket-plug variant {variant_index}");
        Ok(WeaponSocketPlugVariantOverride {
            replace_effects: self.replace_effects,
            investment_stats: self
                .investment_stats
                .iter()
                .map(|stat| (stat.definition_index, stat.value))
                .collect(),
            socket_index: self.socket_index,
            choice_index: self.choice_index,
            source_plug_hash: parse_recipe_hash(
                &format!("{context} source plug"),
                &self.source_plug_hash,
            )?,
            name: self.name.clone(),
            description: self.description.clone(),
            additional_sandbox_perks: self.additional_sandbox_perks.clone(),
            classification_donor_hash: self
                .classification_donor_hash
                .as_ref()
                .map(|hash| parse_recipe_hash(&format!("{context} classification source"), hash))
                .transpose()?,
            sandbox_perks: self
                .sandbox_perks
                .iter()
                .enumerate()
                .map(|(index, perk)| perk.to_compiler(&format!("{context} perk {index}")))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl WeaponSandboxPerkRuntimeRecipe {
    fn to_compiler(&self, context: &str) -> Result<WeaponSandboxPerkRuntimeOverride, RecipeError> {
        Ok(WeaponSandboxPerkRuntimeOverride {
            program: self.program.clone(),
            source_perk_index: self.source_perk_index,
            projectiles: self.projectiles.clone(),
            activation: self.activation,
            runtime_values: self.runtime_values.clone(),
            action_float_values: self
                .action_float_values
                .iter()
                .enumerate()
                .map(|(index, value)| value.to_compiler(&format!("{context} action float {index}")))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl WeaponSandboxPerkActionFloatRecipe {
    fn to_compiler(
        &self,
        context: &str,
    ) -> Result<WeaponSandboxPerkActionFloatOverride, RecipeError> {
        Ok(WeaponSandboxPerkActionFloatOverride {
            node_type_handle: parse_recipe_hash(
                &format!("{context} node type"),
                &self.node_type_handle,
            )?,
            node_occurrence: self.node_occurrence,
            value_pointer_offset: self.value_pointer_offset,
            value_type_handle: parse_recipe_hash(
                &format!("{context} value type"),
                &self.value_type_handle,
            )?,
            expected_bits: self.expected_bits,
            value_bits: self.value_bits,
        })
    }
}
