//! Verified parameter adapters. Recipes still use the compiler's validated native locators.
use super::*;
use sundial::package_authoring::weapon_runtime::WeaponRuntimeRootKind;
mod profiles;
use profiles::{PROJECTILE_PROFILES, ProjectileProfile};

pub(super) const GUIDED_SUPPORT_NOTICE: &str = "Mapped projectile properties include speed, gravity and travel distance for supported assets. Other properties are still being mapped.";

pub(in crate::app) fn has_guided_profile(perk_index: u16) -> bool {
    PROJECTILE_PROFILES
        .iter()
        .any(|profile| profile.perk_index == perk_index)
}

pub(in crate::app) fn guided_support_notice(ui: &mut egui::Ui) {
    ui.label(GUIDED_SUPPORT_NOTICE).on_hover_text(
        "Micro-Missile's speed multiplier has gameplay evidence. Gravity, distance and other projectile combinations are mapped from native code and still need gameplay checks. Stat Bonuses While Equipped are separate from conditional bonuses.",
    );
}

pub(super) struct ProjectileSpeed {
    profile: &'static ProjectileProfile,
    targets: Vec<(WeaponRuntimeField, usize)>,
}

impl ProjectileSpeed {
    #[cfg(test)]
    pub(super) fn discover(loaded: &PrivatePerkRuntimeGraph) -> Option<Self> {
        Self::discover_all(loaded).into_iter().next()
    }

    pub(super) fn discover_all(loaded: &PrivatePerkRuntimeGraph) -> Vec<Self> {
        PROJECTILE_PROFILES
            .iter()
            .filter_map(|profile| Self::resolve(loaded, profile))
            .collect()
    }

    fn resolve(
        loaded: &PrivatePerkRuntimeGraph,
        profile: &'static ProjectileProfile,
    ) -> Option<Self> {
        // An independently opened entity has no parent action. Exact graph, owner,
        // schema, unique field and original value checks below still apply.
        let independent_entity =
            loaded.action_tag == 0 && loaded.action_payload.is_empty() && loaded.graphs.len() == 1;
        if loaded.action_tag != profile.action_tag && !independent_entity {
            return None;
        }
        let mut graphs = loaded
            .graphs
            .iter()
            .filter(|(tag, _)| *tag == profile.graph_tag);
        let (_, graph) = graphs.next()?;
        if graphs.next().is_some() {
            return None;
        }
        let mut targets = Vec::new();
        for (root, schema, offset) in [
            (WeaponRuntimeRootKind::ComponentInstance, 0x8080_3B73, 0x144),
            (
                WeaponRuntimeRootKind::ComponentDefinition,
                0x8080_388F,
                0x88,
            ),
        ] {
            let mut fields = graph.fields().filter(|field| {
                field.locator.root == root
                    && field.locator.root_schema == schema
                    && field.locator.value_offset <= offset
                    && field
                        .locator
                        .value_offset
                        .checked_add(field.locator.byte_size)
                        .is_some_and(|end| end >= offset + 4)
                    && graph.bindings.iter().any(|binding| {
                        binding.binding_hash == field.locator.binding_hash
                            && binding.resource_index == field.locator.resource_index
                            && binding.owner_tag == profile.owner_tag
                    })
            });
            let field = fields.next()?.clone();
            if fields.next().is_some() || !matches!(field.value, WeaponRuntimeValue::Bytes(_)) {
                return None;
            }
            let relative = (offset - field.locator.value_offset) as usize;
            targets.push((field, relative));
        }
        let parameter = Self { profile, targets };
        parameter
            .value(loaded, &[])
            .ok()
            .filter(|value| value.to_bits() == profile.default_multiplier.to_bits())?;
        Some(parameter)
    }

    pub(super) fn value(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<f32, String> {
        let mut values = Vec::new();
        for (field, offset) in &self.targets {
            let value = draft
                .iter()
                .find(|value| equivalent(loaded, &field.locator, &value.locator))
                .map_or(&field.value, |value| &value.value);
            let WeaponRuntimeValue::Bytes(bytes) = value else {
                return Err("Projectile speed has an incompatible saved value.".into());
            };
            let bytes: [u8; 4] = bytes
                .get(*offset..*offset + 4)
                .ok_or("Projectile speed data is truncated.")?
                .try_into()
                .unwrap();
            values.push(f32::from_le_bytes(bytes));
        }
        if values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("Projectile speed must be a finite multiplier greater than zero.".into());
        }
        if values[0].to_bits() != values[1].to_bits() {
            return Err("Projectile prototype and definition speeds disagree. Set a multiplier to update both, or reset them.".into());
        }
        Ok(values[0])
    }

    #[cfg(test)]
    pub(super) fn set(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        value: f32,
    ) -> Result<(), String> {
        if !value.is_finite() || value <= 0.0 {
            return Err("Enter a finite multiplier greater than zero.".into());
        }
        let mut next = draft.clone();
        for (field, offset) in &self.targets {
            let existing = next
                .iter()
                .position(|value| equivalent(loaded, &field.locator, &value.locator));
            let mut entry = existing
                .map(|index| next[index].clone())
                .unwrap_or_else(|| WeaponRuntimeValueOverride {
                    locator: field.locator.clone(),
                    value: field.value.clone(),
                });
            let WeaponRuntimeValue::Bytes(bytes) = &mut entry.value else {
                return Err("Reset incompatible projectile data before editing.".into());
            };
            bytes
                .get_mut(*offset..*offset + 4)
                .ok_or("Projectile data is truncated")?
                .copy_from_slice(&value.to_le_bytes());
            if let Some(index) = existing {
                next.remove(index);
            }
            // Reset only the speed lanes; preserve unrelated custom bytes in the same field.
            if entry.value != field.value {
                next.push(entry);
            }
        }
        *draft = next;
        Ok(())
    }
}

pub(super) fn equivalent(
    loaded: &PrivatePerkRuntimeGraph,
    left: &WeaponRuntimeFieldLocator,
    right: &WeaponRuntimeFieldLocator,
) -> bool {
    let mut normalized = right.clone();
    normalized.binding_hash = left.binding_hash;
    normalized.resource_index = left.resource_index;
    if normalized != *left {
        return false;
    }
    if left.binding_hash == right.binding_hash && left.resource_index == right.resource_index {
        return true;
    }
    loaded.graphs.iter().any(|(_, graph)| {
        let binding = |locator: &WeaponRuntimeFieldLocator| graph.bindings.iter().find(|binding| {
            binding.binding_hash == locator.binding_hash && binding.resource_index == locator.resource_index
        });
        matches!((binding(left), binding(right)), (Some(a), Some(b)) if a.owner_tag == b.owner_tag && a.resource_offset == b.resource_offset)
    })
}

impl PerkEditor {
    pub(super) fn draw_verified_projectile_speed(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        tag: u32,
        owner: u32,
    ) -> bool {
        let parameters = ProjectileSpeed::discover_all(loaded);
        if let Some(speed) = parameters
            .iter()
            .find(|speed| speed.profile.graph_tag == tag && speed.profile.owner_tag == owner)
        {
            ui.label("Projectile Speed Multiplier")
                .on_hover_text(speed.profile.evidence);
            return true;
        }
        false
    }
}
