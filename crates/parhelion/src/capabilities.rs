//! Pure donor capability and override validation rules for Parhelion authoring.
//!
//! This module does not inspect or mutate packages. It turns the investment metadata already
//! decoded by Sundial into authoring choices, structural requirements and compatibility
//! warnings. Unusual plug combinations remain available; a warning is not a build failure.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
use sundial::investment::MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES;
use sundial::investment::{
    MAX_WEAPON_SOCKETS, WeaponDamageProfile, WeaponDamageType, WeaponDonor, WeaponDonorSummary,
    WeaponInventorySlot, authored_socket_choice_limit,
};

use crate::recipe::{RecipeDamageType, RecipeInventorySlot, WeaponRecipe, WeaponRecipeOverrides};

/// The recipe field associated with a capability or validation diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringField {
    Donor,
    InventorySlot,
    DamageProfile,
    InvestmentStat { definition_index: u16 },
    SocketColumn { socket_index: usize },
}

/// A stable, machine-readable explanation for an authoring diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringDiagnosticCode {
    CollectionsBackingRequired,
    MissingInventorySlot,
    MissingEquipmentSlot,
    EquipmentSlotMismatch,
    IncoherentDamageProfile,
    DuplicateStatOverride,
    UnsupportedStatDefinition,
    StatValueBelowMinimum,
    StatValueAboveMaximum,
    SocketCountMismatch,
    MissingAddedSocketType,
    DuplicateSupportedPlugSet,
    SupportedPlugSetSocketIndexOutOfRange,
    MissingSupportedPlugSet,
    UnsupportedPlug,
    EmptySocketColumn,
    TooManySocketColumnChoices,
    DuplicateSocketColumnPlug,
    DisabledSocketOverride,
    ZeroPlugHash,
}

/// A structured diagnostic shared by the UI and compiler validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoringDiagnostic {
    pub field: AuthoringField,
    pub code: AuthoringDiagnosticCode,
    pub message: String,
}

impl AuthoringDiagnostic {
    /// Compatibility membership is advisory: Parhelion can deliberately author a known plug
    /// outside the donor's advertised set. Structural recipe and package failures still block.
    #[must_use]
    pub const fn is_build_blocking(&self) -> bool {
        !matches!(
            self.code,
            AuthoringDiagnosticCode::UnsupportedPlug
                | AuthoringDiagnosticCode::MissingSupportedPlugSet
        )
    }
}

/// A concrete, atomic inventory-slot and damage-type result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatProfile {
    pub inventory_slot: WeaponInventorySlot,
    pub damage_type: WeaponDamageType,
}

impl CombatProfile {
    #[must_use]
    pub fn label(self) -> String {
        format!(
            "{} / {}",
            self.inventory_slot.label(),
            self.damage_type.label()
        )
    }
}

/// The single operation represented by a combat-profile choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatProfileAction {
    /// Leave the donor's slot and damage descriptor byte-for-byte unchanged.
    Preserve,
    /// Request a paired slot and damage override, subject to native build validation.
    Set(CombatProfile),
}

#[must_use]
pub(crate) fn recipe_combat_profile_action(
    overrides: &WeaponRecipeOverrides,
    donor: &WeaponDonorSummary,
) -> Option<CombatProfileAction> {
    if overrides.inventory_slot.is_none() && overrides.modern_damage_type.is_none() {
        return Some(CombatProfileAction::Preserve);
    }
    let inventory_slot = overrides
        .inventory_slot
        .map(recipe_inventory_slot)
        .or(donor.inventory_slot)?;
    let damage_type = overrides
        .modern_damage_type
        .map(recipe_damage_type)
        .or(donor.damage_type)?;
    // Explicitly restating the donor's slot/element is equivalent to preservation.
    // Keep the serialized overrides; only normalize the UI/capability action.
    if donor.inventory_slot == Some(inventory_slot) && donor.damage_type == Some(damage_type) {
        return Some(CombatProfileAction::Preserve);
    }
    Some(CombatProfileAction::Set(CombatProfile {
        inventory_slot,
        damage_type,
    }))
}

pub(crate) fn apply_combat_profile_action(
    overrides: &mut WeaponRecipeOverrides,
    donor: &WeaponDonorSummary,
    action: CombatProfileAction,
) {
    match action {
        CombatProfileAction::Preserve => {
            overrides.inventory_slot = None;
            overrides.modern_damage_type = None;
        }
        CombatProfileAction::Set(profile) => {
            overrides.inventory_slot = (donor.inventory_slot != Some(profile.inventory_slot))
                .then(|| recipe_inventory_slot_from_catalog(profile.inventory_slot));
            overrides.modern_damage_type =
                Some(recipe_damage_type_from_catalog(profile.damage_type));
        }
    }
}

#[must_use]
pub(crate) fn authored_inventory_slot(
    overrides: &WeaponRecipeOverrides,
    donor: &WeaponDonorSummary,
) -> Option<WeaponInventorySlot> {
    match recipe_combat_profile_action(overrides, donor)? {
        CombatProfileAction::Preserve => donor.inventory_slot,
        CombatProfileAction::Set(profile) => Some(profile.inventory_slot),
    }
}

#[must_use]
pub(crate) fn presentation_donor_candidate_is_compatible(
    candidate: &WeaponDonorSummary,
    gameplay_donor: &WeaponDonorSummary,
    target_slot: WeaponInventorySlot,
) -> bool {
    candidate.hash != gameplay_donor.hash
        && appearance_compatibility(candidate, gameplay_donor, target_slot)
            == AppearanceCompatibility::Compatible
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppearanceCompatibility {
    Compatible,
    Blocked(&'static str),
    Unchecked,
}

pub(crate) fn effective_weapon_translation_group(
    base: &WeaponDonorSummary,
    pattern: Option<u16>,
    donors: &[WeaponDonorSummary],
) -> Option<u32> {
    match pattern {
        None => base.weapon_translation_group,
        Some(index) => donors
            .iter()
            .find(|donor| donor.weapon_pattern_index == Some(index))
            .and_then(|donor| donor.weapon_translation_group),
    }
}

pub(crate) fn appearance_compatibility(
    candidate: &WeaponDonorSummary,
    base: &WeaponDonorSummary,
    target: WeaponInventorySlot,
) -> AppearanceCompatibility {
    use sundial::package_authoring::native_weapon::{
        AnimationCompatibility, animation_compatibility,
    };
    if !candidate.collection_backed {
        return AppearanceCompatibility::Blocked("Appearance has no Collections entry");
    }
    if candidate.type_name != base.type_name {
        return AppearanceCompatibility::Blocked("Different weapon family");
    }
    if candidate.inventory_slot != Some(target)
        && !matches!(
            (candidate.inventory_slot, target),
            (
                Some(WeaponInventorySlot::Kinetic),
                WeaponInventorySlot::Energy
            ) | (
                Some(WeaponInventorySlot::Energy),
                WeaponInventorySlot::Kinetic
            )
        )
    {
        return AppearanceCompatibility::Blocked("Different inventory slot");
    }
    // Kinetic/Energy placement is an independent bucket field. Cross-slot
    // appearance still requires a known, identical native animation group.
    match animation_compatibility(
        base.weapon_translation_group,
        candidate.weapon_translation_group,
    ) {
        AnimationCompatibility::Compatible => AppearanceCompatibility::Compatible,
        AnimationCompatibility::DifferentGroups => {
            AppearanceCompatibility::Blocked("Incompatible weapon animations")
        }
        AnimationCompatibility::Unchecked => AppearanceCompatibility::Unchecked,
    }
}

#[must_use]
pub(crate) fn selected_presentation_donor_is_compatible(
    recipe: &WeaponRecipe,
    gameplay_donor: &WeaponDonorSummary,
    donor_summaries: &[WeaponDonorSummary],
) -> bool {
    let mut effective_base = gameplay_donor.clone();
    effective_base.weapon_translation_group = effective_weapon_translation_group(
        gameplay_donor,
        recipe.overrides.weapon_pattern_index,
        donor_summaries,
    );
    let Some(reference) = recipe.presentation_donor.as_ref() else {
        return false;
    };
    let Ok(hash) = reference.item_hash.parse_u32() else {
        return false;
    };
    let Some(target_slot) = authored_inventory_slot(&recipe.overrides, gameplay_donor) else {
        return false;
    };
    donor_summaries
        .iter()
        .find(|candidate| candidate.hash == hash)
        .is_some_and(|candidate| {
            appearance_compatibility(candidate, &effective_base, target_slot)
                == AppearanceCompatibility::Compatible
        })
}

pub(crate) fn reconcile_presentation_donor(
    recipe: &mut WeaponRecipe,
    gameplay_donor: &WeaponDonorSummary,
    donor_summaries: &[WeaponDonorSummary],
) {
    if recipe.presentation_donor.is_some()
        && !selected_presentation_donor_is_compatible(recipe, gameplay_donor, donor_summaries)
    {
        recipe.set_presentation_donor(None);
    }
}

const fn recipe_inventory_slot(value: RecipeInventorySlot) -> WeaponInventorySlot {
    match value {
        RecipeInventorySlot::Kinetic => WeaponInventorySlot::Kinetic,
        RecipeInventorySlot::Energy => WeaponInventorySlot::Energy,
        RecipeInventorySlot::Power => WeaponInventorySlot::Power,
    }
}

pub(crate) const fn recipe_inventory_slot_from_catalog(
    value: WeaponInventorySlot,
) -> RecipeInventorySlot {
    match value {
        WeaponInventorySlot::Kinetic => RecipeInventorySlot::Kinetic,
        WeaponInventorySlot::Energy => RecipeInventorySlot::Energy,
        WeaponInventorySlot::Power => RecipeInventorySlot::Power,
    }
}

const fn recipe_damage_type(value: RecipeDamageType) -> WeaponDamageType {
    match value {
        RecipeDamageType::Kinetic => WeaponDamageType::Kinetic,
        RecipeDamageType::Arc => WeaponDamageType::Arc,
        RecipeDamageType::Solar => WeaponDamageType::Solar,
        RecipeDamageType::Void => WeaponDamageType::Void,
    }
}

pub(crate) const fn recipe_damage_type_from_catalog(value: WeaponDamageType) -> RecipeDamageType {
    match value {
        WeaponDamageType::Kinetic => RecipeDamageType::Kinetic,
        WeaponDamageType::Arc => RecipeDamageType::Arc,
        WeaponDamageType::Solar => RecipeDamageType::Solar,
        WeaponDamageType::Void => RecipeDamageType::Void,
    }
}

/// A user-facing choice with the safety reason that caused it to be offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombatProfileChoice {
    pub action: CombatProfileAction,
    pub label: String,
    pub reason: String,
}

/// Conservative authoring capabilities for one installed weapon donor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponAuthoringCapabilities {
    pub combat_profiles: Vec<CombatProfileChoice>,
    pub diagnostics: Vec<AuthoringDiagnostic>,
}

impl WeaponAuthoringCapabilities {
    #[must_use]
    pub fn is_authorable(&self) -> bool {
        self.diagnostics.is_empty() && !self.combat_profiles.is_empty()
    }

    #[must_use]
    pub fn supports(&self, action: CombatProfileAction) -> bool {
        self.is_authorable()
            && self
                .combat_profiles
                .iter()
                .any(|choice| choice.action == action)
    }
}

/// Computes metadata-based authoring choices and checks the donor's equipment-slot coherence.
/// Compilation additionally validates native carrier topology and compatible donor availability.
#[must_use]
pub fn weapon_authoring_capabilities(donor: &WeaponDonor) -> WeaponAuthoringCapabilities {
    let mut capabilities = weapon_summary_authoring_capabilities(&donor.summary);
    if let Some(inventory_slot) = donor.summary.inventory_slot {
        match donor.equipment_slot {
            Some(equipment_slot) if inventory_slot != equipment_slot => {
                capabilities.diagnostics.push(AuthoringDiagnostic {
                    field: AuthoringField::InventorySlot,
                    code: AuthoringDiagnosticCode::EquipmentSlotMismatch,
                    message: format!(
                        "{} has a {} inventory bucket but a {} equipment slot",
                        donor.summary.name,
                        inventory_slot.label(),
                        equipment_slot.label(),
                    ),
                });
                capabilities.combat_profiles.clear();
            }
            None => {
                capabilities.diagnostics.push(AuthoringDiagnostic {
                    field: AuthoringField::InventorySlot,
                    code: AuthoringDiagnosticCode::MissingEquipmentSlot,
                    message: format!(
                        "{} has a decoded inventory bucket but no recognized equipment slot",
                        donor.summary.name
                    ),
                });
                capabilities.combat_profiles.clear();
            }
            _ => {}
        }
    }
    capabilities
}

/// Computes authoring choices from summary metadata without inspecting native payload topology.
#[must_use]
pub fn weapon_summary_authoring_capabilities(
    donor: &WeaponDonorSummary,
) -> WeaponAuthoringCapabilities {
    let mut diagnostics = Vec::new();
    if !donor.collection_backed {
        diagnostics.push(AuthoringDiagnostic {
            field: AuthoringField::Donor,
            code: AuthoringDiagnosticCode::CollectionsBackingRequired,
            message: format!(
                "{} is not backed by a Collections definition, so Parhelion cannot safely author from it",
                donor.name
            ),
        });
    }

    let Some(slot) = donor.inventory_slot else {
        diagnostics.push(AuthoringDiagnostic {
            field: AuthoringField::InventorySlot,
            code: AuthoringDiagnosticCode::MissingInventorySlot,
            message: format!(
                "{} does not have a decoded Kinetic, Energy, or Power inventory slot",
                donor.name
            ),
        });
        return WeaponAuthoringCapabilities {
            combat_profiles: Vec::new(),
            diagnostics,
        };
    };

    if !profile_is_coherent(donor) {
        diagnostics.push(AuthoringDiagnostic {
            field: AuthoringField::DamageProfile,
            code: AuthoringDiagnosticCode::IncoherentDamageProfile,
            message: format!(
                "{} has conflicting decoded damage metadata; preserving or mutating it is unsafe",
                donor.name
            ),
        });
        return WeaponAuthoringCapabilities {
            combat_profiles: Vec::new(),
            diagnostics,
        };
    }

    let mut combat_profiles = vec![preserve_choice(donor, slot)];
    let damage_types: &[WeaponDamageType] = match donor.damage_profile {
        WeaponDamageProfile::KineticEmpty
        | WeaponDamageProfile::ModernFixed(_)
        | WeaponDamageProfile::LegacyFixed(_) => &[
            WeaponDamageType::Kinetic,
            WeaponDamageType::Arc,
            WeaponDamageType::Solar,
            WeaponDamageType::Void,
        ],
        WeaponDamageProfile::PlugOrEmptyAmbiguous(Some(current)) if is_elemental(current) => &[
            WeaponDamageType::Arc,
            WeaponDamageType::Solar,
            WeaponDamageType::Void,
        ],
        _ => &[],
    };
    for inventory_slot in [
        WeaponInventorySlot::Kinetic,
        WeaponInventorySlot::Energy,
        WeaponInventorySlot::Power,
    ] {
        for &damage_type in damage_types {
            if inventory_slot == slot && donor.damage_type == Some(damage_type) {
                continue;
            }
            let profile = CombatProfile {
                inventory_slot,
                damage_type,
            };
            combat_profiles.push(CombatProfileChoice {
                action: CombatProfileAction::Set(profile),
                label: profile.label(),
                reason: concat!(
                    "Slot and damage are independent authoring fields. Existing elemental carriers ",
                    "keep their topology; adding a carrier requires a compatible stock source. ",
                    "Slot changes can retain the base appearance and animations. Unusual pairings need gameplay testing."
                ).to_owned(),
            });
        }
    }

    WeaponAuthoringCapabilities {
        combat_profiles,
        diagnostics,
    }
}

fn preserve_choice(donor: &WeaponDonorSummary, slot: WeaponInventorySlot) -> CombatProfileChoice {
    let profile_label = match (slot, donor.damage_type) {
        (WeaponInventorySlot::Kinetic, Some(WeaponDamageType::Kinetic)) => "Kinetic".to_owned(),
        (_, Some(damage_type)) => CombatProfile {
            inventory_slot: slot,
            damage_type,
        }
        .label(),
        (_, None) => slot.label().to_owned(),
    };
    CombatProfileChoice {
        action: CombatProfileAction::Preserve,
        label: format!("{profile_label} (base weapon)"),
        reason: concat!(
            "Safe clone: keep the donor's inventory slot and complete damage representation ",
            "byte-for-byte."
        )
        .to_owned(),
    }
}

fn profile_is_coherent(donor: &WeaponDonorSummary) -> bool {
    let profile_damage_type = match donor.damage_profile {
        WeaponDamageProfile::KineticEmpty => Some(WeaponDamageType::Kinetic),
        WeaponDamageProfile::ModernFixed(damage_type)
        | WeaponDamageProfile::LegacyFixed(damage_type) => Some(damage_type),
        WeaponDamageProfile::PlugOrEmptyAmbiguous(damage_type) => damage_type,
        WeaponDamageProfile::Variable | WeaponDamageProfile::Unknown => donor.damage_type,
    };

    let Some(damage_type) = profile_damage_type else {
        // An ambiguous or unknown representation can still be cloned safely because Preserve does
        // not reinterpret its bytes. A decoded slot is nevertheless required by the contract.
        return true;
    };
    if donor
        .damage_type
        .is_some_and(|summary_type| summary_type != damage_type)
    {
        return false;
    }
    true
}

fn is_elemental(damage_type: WeaponDamageType) -> bool {
    matches!(
        damage_type,
        WeaponDamageType::Arc | WeaponDamageType::Solar | WeaponDamageType::Void
    )
}

/// Validates stat changes against native donor rows and installed weapon-row definitions that can
/// be appended to the selected donor.
#[must_use]
pub fn validate_stat_overrides(
    donor: &WeaponDonor,
    overrides: &[(u16, i32)],
) -> Vec<AuthoringDiagnostic> {
    let supported = donor
        .investment_stats
        .iter()
        .chain(&donor.addable_investment_stats)
        .map(|stat| (stat.definition_index, stat))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut diagnostics = Vec::new();

    for &(definition_index, value) in overrides {
        let field = AuthoringField::InvestmentStat { definition_index };
        if !seen.insert(definition_index) {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::DuplicateStatOverride,
                message: format!(
                    "Investment stat definition {definition_index} is overridden more than once"
                ),
            });
        }
        let Some(stat) = supported.get(&definition_index).copied() else {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::UnsupportedStatDefinition,
                message: format!(
                    "Investment stat definition {definition_index} is not present in {} and has no installed weapon-row definition",
                    donor.summary.name
                ),
            });
            continue;
        };
        if let Some(minimum) = stat.minimum_value {
            if value < minimum {
                diagnostics.push(AuthoringDiagnostic {
                    field,
                    code: AuthoringDiagnosticCode::StatValueBelowMinimum,
                    message: format!(
                        "Investment stat definition {definition_index} value {value} is below its decoded minimum {minimum}"
                    ),
                });
            }
        }
        if let Some(maximum) = stat.maximum_value {
            if value > maximum {
                diagnostics.push(AuthoringDiagnostic {
                    field,
                    code: AuthoringDiagnosticCode::StatValueAboveMaximum,
                    message: format!(
                        "Investment stat definition {definition_index} value {value} exceeds the donor stat-group maximumValue {maximum}"
                    ),
                });
            }
        }
    }
    diagnostics
}

/// Installed compatible-plug hashes for one donor socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupportedPlugSet {
    pub socket_index: usize,
    pub plug_hashes: Vec<u32>,
}

/// Validates complete positional socket-column overrides against installed plug pools.
///
/// An empty override inherits the donor unchanged. A non-empty override must be positional and
/// complete. Package/catalog discovery intentionally stays outside this pure module. Callers must
/// pass the compatible set decoded for each authored socket. Membership findings are safety
/// warnings; structural column errors remain build-blocking.
#[must_use]
#[cfg(test)]
pub fn validate_socket_column_overrides(
    donor: &WeaponDonor,
    overrides: &[Option<Vec<u32>>],
    supported_plug_sets: &[SupportedPlugSet],
) -> Vec<AuthoringDiagnostic> {
    validate_socket_column_overrides_with_socket_types(donor, overrides, &[], supported_plug_sets)
}

/// Validates socket choices after applying any explicit native socket-type overrides.
#[must_use]
pub fn validate_socket_column_overrides_with_socket_types(
    donor: &WeaponDonor,
    overrides: &[Option<Vec<u32>>],
    socket_types: &[Option<u16>],
    supported_plug_sets: &[SupportedPlugSet],
) -> Vec<AuthoringDiagnostic> {
    let mut diagnostics = Vec::new();
    if overrides.is_empty() {
        return diagnostics;
    }
    if overrides.len() < donor.sockets.len() || overrides.len() > MAX_WEAPON_SOCKETS {
        diagnostics.push(AuthoringDiagnostic {
            field: AuthoringField::SocketColumn {
                socket_index: overrides.len().min(donor.sockets.len()),
            },
            code: AuthoringDiagnosticCode::SocketCountMismatch,
            message: format!(
                "Socket-column override has {} rows. {} requires at least {} and supports at most {MAX_WEAPON_SOCKETS} sockets",
                overrides.len(),
                donor.summary.name,
                donor.sockets.len()
            ),
        });
    }

    let (plug_sets, invalid_plug_set_indices) = validate_supported_plug_sets(
        donor,
        donor
            .sockets
            .len()
            .max(overrides.len())
            .min(MAX_WEAPON_SOCKETS),
        supported_plug_sets,
        &mut diagnostics,
    );
    for (socket_index, value) in overrides.iter().enumerate().take(MAX_WEAPON_SOCKETS) {
        if invalid_plug_set_indices.contains(&socket_index) {
            continue;
        }
        validate_socket_column(
            donor,
            socket_index,
            value.as_deref(),
            socket_types.get(socket_index).copied().flatten(),
            plug_sets.get(&socket_index).copied(),
            &mut diagnostics,
        );
    }
    diagnostics
}

fn validate_supported_plug_sets<'a>(
    donor: &WeaponDonor,
    socket_count: usize,
    supported_plug_sets: &'a [SupportedPlugSet],
    diagnostics: &mut Vec<AuthoringDiagnostic>,
) -> (BTreeMap<usize, &'a SupportedPlugSet>, BTreeSet<usize>) {
    let mut plug_sets_by_index = BTreeMap::<_, Vec<_>>::new();
    for set in supported_plug_sets {
        plug_sets_by_index
            .entry(set.socket_index)
            .or_default()
            .push(set);
    }
    let mut plug_sets = BTreeMap::new();
    let mut invalid_plug_set_indices = BTreeSet::new();
    for (socket_index, sets) in plug_sets_by_index {
        let field = AuthoringField::SocketColumn { socket_index };
        if socket_index >= socket_count {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::SupportedPlugSetSocketIndexOutOfRange,
                message: format!(
                    "Compatible-plug set socket index {socket_index} is outside {}'s {} sockets",
                    donor.summary.name, socket_count
                ),
            });
            invalid_plug_set_indices.insert(socket_index);
        }
        if sets.len() > 1 {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::DuplicateSupportedPlugSet,
                message: format!(
                    "Compatible-plug set socket index {socket_index} is supplied {} times",
                    sets.len()
                ),
            });
            invalid_plug_set_indices.insert(socket_index);
        }
        if !invalid_plug_set_indices.contains(&socket_index) {
            plug_sets.insert(socket_index, sets[0]);
        }
    }
    (plug_sets, invalid_plug_set_indices)
}

fn validate_socket_column(
    donor: &WeaponDonor,
    socket_index: usize,
    choices: Option<&[u32]>,
    socket_type_override: Option<u16>,
    supported: Option<&SupportedPlugSet>,
    diagnostics: &mut Vec<AuthoringDiagnostic>,
) {
    let field = AuthoringField::SocketColumn { socket_index };
    let socket = donor.sockets.get(socket_index);
    if socket.is_none() && socket_type_override.is_none() {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::MissingAddedSocketType,
            message: format!("Added socket {socket_index} requires an explicit socket type"),
        });
    }
    let Some(choices) = choices else {
        if socket.is_none() {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::EmptySocketColumn,
                message: format!("Added socket {socket_index} must contain at least one plug"),
            });
        }
        return;
    };
    let maximum = socket_type_override.map_or_else(
        || socket.map_or(0, |socket| socket.max_authored_choices),
        authored_socket_choice_limit,
    );
    if maximum == 0 {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::DisabledSocketOverride,
            message: format!(
                "Socket {socket_index} is disabled or not authorable in {}",
                donor.summary.name
            ),
        });
        return;
    }
    if choices.is_empty() {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::EmptySocketColumn,
            message: format!("Socket {socket_index} must contain at least one plug"),
        });
        return;
    }
    if choices.len() > maximum {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::TooManySocketColumnChoices,
            message: format!(
                "Socket {socket_index} accepts at most {} authored choice{}, but {} were provided",
                maximum,
                if maximum == 1 { "" } else { "s" },
                choices.len()
            ),
        });
    }
    validate_socket_choice_values(socket_index, choices, supported, diagnostics);
}

fn validate_socket_choice_values(
    socket_index: usize,
    choices: &[u32],
    supported: Option<&SupportedPlugSet>,
    diagnostics: &mut Vec<AuthoringDiagnostic>,
) {
    let field = AuthoringField::SocketColumn { socket_index };
    let mut seen = BTreeSet::new();
    for &hash in choices {
        if hash == 0 {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::ZeroPlugHash,
                message: format!("Socket {socket_index} cannot use plug hash zero"),
            });
        } else if !seen.insert(hash) {
            diagnostics.push(AuthoringDiagnostic {
                field,
                code: AuthoringDiagnosticCode::DuplicateSocketColumnPlug,
                message: format!("Socket {socket_index} contains plug 0x{hash:08X} more than once"),
            });
        }
    }
    let Some(supported) = supported else {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::MissingSupportedPlugSet,
            message: format!(
                "Socket {socket_index} has no decoded compatible-plug set, so its choices cannot be verified"
            ),
        });
        return;
    };
    let supported_hashes = supported
        .plug_hashes
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for &hash in choices
        .iter()
        .filter(|&&hash| hash != 0)
        .filter(|&&hash| !supported_hashes.contains(&hash))
    {
        diagnostics.push(AuthoringDiagnostic {
            field,
            code: AuthoringDiagnosticCode::UnsupportedPlug,
            message: format!(
                "Plug 0x{hash:08X} is outside the base weapon's compatible set for socket {socket_index}. Test its behavior in game."
            ),
        });
    }
}

#[cfg(test)]
mod tests;
