//! What an authored item's icon shows in place of its image: a Sparrow's summoned vehicle as that
//! vehicle's HUD silhouette, or a subclass's generated icon, a diamond in its subclass color. An
//! icon image of the author's own always wins. The build draws the art from the item's sources.
//! The page's icon card, its icon editors and library rows draw it through a [`Request`], which
//! names each Super by the entity the catalog's subclasses give it.
use sundial::investment::SubclassSummary;
use sundial::package_authoring::PackageManager;

use crate::icon_edit::ImportedIcon;
pub(crate) use crate::subclass::icon::Look;
use crate::subclass::icon::{Generated, Parts, Preview};
use crate::vehicle::Summon;

/// `edit` with `art`, which `request` drew, as the icon's image. A silhouette takes the icon's own
/// edits, as the image it replaces would. A generated icon is drawn whole in its subclass color, so
/// the edits would only recolor it.
pub(crate) fn edited(
    request: Option<&Request>,
    edit: &crate::WeaponIconEdit,
    art: Option<&ImportedIcon>,
) -> crate::WeaponIconEdit {
    match (request, art) {
        (Some(Request::Generated(_)), Some(art)) => crate::WeaponIconEdit::drawn(art),
        _ => edit.with_art(art),
    }
}

/// The art as a recipe gives it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Art {
    Vehicle(Summon),
    Generated(Generated),
}

impl Art {
    /// The art `recipe`'s icon shows in place of its image, if any.
    pub(crate) fn of(recipe: &crate::WeaponRecipe) -> Option<Self> {
        let overrides = &recipe.overrides;
        if let Some(vehicle) =
            crate::vehicle::icon::shown(overrides.sparrow.as_ref(), &overrides.icon_edit)
        {
            return Some(Self::Vehicle(vehicle.clone()));
        }
        Generated::of(recipe).map(Self::Generated)
    }

    /// What a preview reads and draws for the art, with each Super's entity as `subclasses` name
    /// it.
    pub(crate) fn request(&self, subclasses: &[SubclassSummary]) -> Request {
        match self {
            Self::Vehicle(vehicle) => Request::Vehicle(vehicle.clone()),
            Self::Generated(generated) => Request::Generated(generated.preview(subclasses)),
        }
    }
}

/// The art as a preview reads and draws it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    Vehicle(Summon),
    Generated(Preview),
}

impl Request {
    /// Whether both read the same from the packages. A generated icon's color and symbol size are
    /// drawn, not read, so a preview redraws it without reading the packages again.
    pub(crate) fn same_reads(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Generated(generated), Self::Generated(other)) => generated.same_reads(other),
            _ => self == other,
        }
    }

    /// How a generated icon draws: in its own HUD color, `None` taking the Super's, with its
    /// symbol at its size.
    pub(crate) fn look(&self) -> Look {
        match self {
            Self::Vehicle(_) => Look::default(),
            Self::Generated(generated) => generated.look,
        }
    }

    /// Reads what the art needs from the packages.
    pub(crate) fn read(&self, manager: &PackageManager) -> Result<Read, String> {
        Ok(match self {
            Self::Vehicle(vehicle) => {
                Read::Silhouette(crate::vehicle::icon::silhouette(manager, vehicle)?)
            }
            Self::Generated(generated) => Read::Generated(generated.read(manager)?),
        })
    }

    /// Reads the art and draws it.
    pub(crate) fn draw(&self, manager: &PackageManager) -> Result<Option<ImportedIcon>, String> {
        self.read(manager)?.draw(self.look())
    }
}

/// The art as read: a silhouette whole, or a generated icon's parts, which draw in its color.
#[derive(Clone, Debug)]
pub(crate) enum Read {
    /// `None` for a vehicle whose weapon names no HUD icon, such as an unarmed one.
    Silhouette(Option<ImportedIcon>),
    Generated(Parts),
}

impl Read {
    /// The art, a generated icon drawn as `look` draws it.
    pub(crate) fn draw(&self, look: Look) -> Result<Option<ImportedIcon>, String> {
        match self {
            Self::Silhouette(silhouette) => Ok(silhouette.clone()),
            Self::Generated(parts) => parts.draw(look).map(Some),
        }
    }
}
