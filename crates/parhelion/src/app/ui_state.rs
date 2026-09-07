//! Pure state and sizing decisions shared by the Parhelion editor UI.

/// The main page contains the complete everyday weapon-authoring workflow.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum WorkbenchPage {
    #[default]
    Weapon,
    Appearance,
    Advanced,
    Identity,
}

impl WorkbenchPage {
    pub(crate) const ALL: [Self; 4] = [
        Self::Weapon,
        Self::Appearance,
        Self::Advanced,
        Self::Identity,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Appearance => "Appearance",
            Self::Advanced => "Advanced Gameplay",
            Self::Identity => "Identity",
        }
    }
}

/// Shared edge for the donor/stat column and the definition/socket column.
pub(crate) fn workbench_left_column_width(width: f32) -> Option<f32> {
    (width.is_finite() && width >= 1120.0).then_some((width * 0.34).floor())
}

/// Stable sub-pages for the technical gameplay editor.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum AdvancedGameplayPage {
    #[default]
    Runtime,
    PerksTraits,
    Inventory,
    Raw,
}

impl AdvancedGameplayPage {
    pub(crate) const ALL: [Self; 4] =
        [Self::Runtime, Self::PerksTraits, Self::Inventory, Self::Raw];
    pub(crate) const STABLE: [Self; 1] = [Self::Runtime];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Runtime => "Runtime",
            Self::PerksTraits => "Perks & Traits",
            Self::Inventory => "Inventory",
            Self::Raw => "Raw",
        }
    }
}

/// Consecutive pages in the single build/install window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum BuildDialogStep {
    #[default]
    Build,
    ReviewInstall,
    Install,
}

/// User-facing persistence state for the active recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecipeSaveStatus {
    NotSavedYet,
    UnsavedChanges,
    Saved,
}

impl RecipeSaveStatus {
    pub(crate) const fn derive(has_library_path: bool, dirty: bool) -> Self {
        if dirty {
            Self::UnsavedChanges
        } else if has_library_path {
            Self::Saved
        } else {
            Self::NotSavedYet
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::NotSavedYet => "Not saved yet",
            Self::UnsavedChanges => "Unsaved changes",
            Self::Saved => "Saved",
        }
    }
}

/// Layout used for one runtime-value editor row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeEditorLayout {
    Inline,
    Stacked,
}

impl RuntimeEditorLayout {
    /// Complex editors always stack; simple editors stack when horizontal space is constrained.
    pub(crate) fn choose(available_width: f32, complex: bool) -> Self {
        const INLINE_MIN_WIDTH: f32 = 720.0;

        if complex || !available_width.is_finite() || available_width < INLINE_MIN_WIDTH {
            Self::Stacked
        } else {
            Self::Inline
        }
    }
}

/// Returns the donor-column width when the runtime workspace has room for a useful split view.
pub(crate) fn runtime_workspace_donor_width(available_width: f32) -> Option<f32> {
    const SPLIT_MIN_WIDTH: f32 = 1_180.0;
    const DONOR_COLUMN_FRACTION: f32 = 0.4;

    (available_width.is_finite() && available_width >= SPLIT_MIN_WIDTH)
        .then_some(available_width * DONOR_COLUMN_FRACTION)
}

/// Reserves room between content and the shared vertical scrollbar.
pub(crate) fn safe_content_width(available_width: f32) -> f32 {
    const RIGHT_INSET: f32 = 16.0;

    if available_width.is_finite() {
        (available_width - RIGHT_INSET).max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbench_header_stacks_before_donor_cards_or_profile_controls_are_cramped() {
        assert_eq!(workbench_left_column_width(900.0), None);
        assert_eq!(workbench_left_column_width(f32::NAN), None);
        assert_eq!(workbench_left_column_width(1280.0), Some(435.0));
    }

    #[test]
    fn save_status_distinguishes_pristine_dirty_and_saved_recipes() {
        assert_eq!(
            RecipeSaveStatus::derive(false, false),
            RecipeSaveStatus::NotSavedYet
        );
        assert_eq!(
            RecipeSaveStatus::derive(false, true),
            RecipeSaveStatus::UnsavedChanges
        );
        assert_eq!(
            RecipeSaveStatus::derive(true, true),
            RecipeSaveStatus::UnsavedChanges
        );
        assert_eq!(
            RecipeSaveStatus::derive(true, false),
            RecipeSaveStatus::Saved
        );
        assert_eq!(RecipeSaveStatus::NotSavedYet.label(), "Not saved yet");
        assert_eq!(RecipeSaveStatus::UnsavedChanges.label(), "Unsaved changes");
        assert_eq!(RecipeSaveStatus::Saved.label(), "Saved");
    }

    #[test]
    fn runtime_editor_stacks_complex_or_narrow_fields() {
        assert_eq!(
            RuntimeEditorLayout::choose(1_200.0, true),
            RuntimeEditorLayout::Stacked
        );
        assert_eq!(
            RuntimeEditorLayout::choose(719.0, false),
            RuntimeEditorLayout::Stacked
        );
        assert_eq!(
            RuntimeEditorLayout::choose(720.0, false),
            RuntimeEditorLayout::Inline
        );
        assert_eq!(
            RuntimeEditorLayout::choose(f32::NAN, false),
            RuntimeEditorLayout::Stacked
        );
    }

    #[test]
    fn runtime_workspace_splits_only_when_both_columns_remain_useful() {
        assert_eq!(runtime_workspace_donor_width(1_179.0), None);
        assert_eq!(runtime_workspace_donor_width(1_180.0), Some(472.0));
        assert_eq!(runtime_workspace_donor_width(1_320.0), Some(528.0));
        assert_eq!(runtime_workspace_donor_width(f32::NAN), None);
    }

    #[test]
    fn safe_width_reserves_the_scrollbar_inset_without_underflow() {
        assert_eq!(safe_content_width(900.0), 884.0);
        assert_eq!(safe_content_width(10.0), 0.0);
        assert_eq!(safe_content_width(f32::INFINITY), 0.0);
    }
}
