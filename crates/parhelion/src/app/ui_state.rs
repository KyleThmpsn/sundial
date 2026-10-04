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
            Self::Advanced => "Gameplay",
            Self::Identity => "Identity",
        }
    }

    /// The pages a kind uses. Gear keeps its base item's geometry and runtime, so it has no
    /// Appearance or Advanced Gameplay page.
    pub(crate) const fn for_kind(kind: crate::ItemKind) -> &'static [Self] {
        match kind {
            crate::ItemKind::Weapon => &Self::ALL,
            // A subclass has no Collections entry. Its Appearance holds its screen art.
            crate::ItemKind::Subclass => &[Self::Weapon, Self::Appearance, Self::Identity],
            _ => &[Self::Weapon, Self::Collections, Self::Identity],
        }
    }

    /// The first page is named for what the recipe builds.
    pub(crate) const fn label_for(self, kind: crate::ItemKind) -> &'static str {
        match self {
            Self::Weapon => kind.label(),
            page => page.label(),
        }
    }
}

/// Shared edge for the donor/stat column and the definition/socket column: three tenths of
/// the width within bounds, so a wider window gives its room to the columns beside it. The
/// columns stack only below the window's narrowest width.
const LEFT_COLUMN_SHARE: f32 = 0.30;
const LEFT_COLUMN_MIN_WIDTH: f32 = 320.0;
const LEFT_COLUMN_MAX_WIDTH: f32 = 420.0;
const COLUMNS_MIN_WIDTH: f32 = 860.0;

pub(crate) fn workbench_left_column_width(width: f32) -> Option<f32> {
    (width.is_finite() && width >= COLUMNS_MIN_WIDTH).then(|| {
        (width * LEFT_COLUMN_SHARE)
            .clamp(LEFT_COLUMN_MIN_WIDTH, LEFT_COLUMN_MAX_WIDTH)
            .floor()
    })
}

/// The model preview beside an item's text. The text keeps `DEFINITION_WIDTH` and the preview
/// takes the rest within its bounds. When that would leave the text narrower than it can go,
/// there is no preview column and the preview goes under the text.
const DEFINITION_MIN_WIDTH: f32 = 420.0;
const DEFINITION_WIDTH: f32 = 540.0;
const PREVIEW_MIN_WIDTH: f32 = 300.0;
const PREVIEW_MAX_WIDTH: f32 = 560.0;
const PREVIEW_MIN_HEIGHT: f32 = 160.0;
const PREVIEW_MAX_HEIGHT: f32 = 300.0;

/// The preview column's width within `beside`, the space beside the left column, with
/// `spacing` between it and the text.
pub(crate) fn preview_column_width(beside: f32, spacing: f32) -> Option<f32> {
    let beside = beside - spacing;
    (beside - DEFINITION_MIN_WIDTH >= PREVIEW_MIN_WIDTH).then(|| {
        (beside - DEFINITION_WIDTH)
            .clamp(PREVIEW_MIN_WIDTH, PREVIEW_MAX_WIDTH)
            .floor()
    })
}

/// The preview's height at `width`: the left column's height `band` within bounds and never
/// flatter than a wide frame needs, or its own proportion when it stands alone under the text.
pub(crate) fn preview_height(width: f32, band: Option<f32>) -> f32 {
    band.map_or_else(
        || (width * 0.75).clamp(220.0, 340.0),
        |band| {
            band.clamp(PREVIEW_MIN_HEIGHT, PREVIEW_MAX_HEIGHT)
                .max((width * 0.45).min(PREVIEW_MAX_HEIGHT))
        },
    )
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
            Self::NotSavedYet => "Not Saved",
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

/// Reserves room between content and the shared vertical scrollbar.
pub(crate) fn safe_content_width(available_width: f32) -> f32 {
    const RIGHT_INSET: f32 = 16.0;

    if available_width.is_finite() {
        (available_width - RIGHT_INSET).max(0.0)
    } else {
        0.0
    }
}
