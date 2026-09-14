//! Pure state and sizing decisions shared by the Parhelion editor UI.

/// The main page contains the complete everyday weapon-authoring workflow.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum WorkbenchPage {
    #[default]
    Weapon,
    Appearance,
    Collections,
    Advanced,
    Identity,
}

impl WorkbenchPage {
    pub(crate) const ALL: [Self; 5] = [
        Self::Weapon,
        Self::Appearance,
        Self::Collections,
        Self::Advanced,
        Self::Identity,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Appearance => "Appearance",
            Self::Collections => "Collections",
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
            Self::UnsavedChanges => "Unsaved Changes",
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
