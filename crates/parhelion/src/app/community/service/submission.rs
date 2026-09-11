//! Anonymous JSON uploads. Publication is managed by the server owner.
use super::Listing;
use crate::WeaponRecipe;

#[derive(Clone)]
pub(crate) struct Submission {
    pub listing: Listing,
    pub recipe: WeaponRecipe,
}
