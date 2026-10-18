//! Without the D2 model importer a build holds no maps, as their worlds and tables need its
//! conversions.
use std::path::Path;

use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

use crate::{AuthoringResult, NewTagSpec, ReplacementSpec, error::invalid, format::NamedTag};

use super::MapRecipe;

/// One of a map's packages. A plan without maps names neither.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Part {
    World,
    Activity,
}

/// An empty plan.
#[derive(Default)]
pub(crate) struct Plan {
    pub replacements: Vec<ReplacementSpec>,
}

impl Plan {
    /// Refuses any map.
    pub(crate) fn load(
        _directory: &Path,
        _manager: &PackageManager,
        recipes: Vec<MapRecipe>,
        _string_floor: u32,
    ) -> AuthoringResult<Self> {
        if recipes.is_empty() {
            Ok(Self::default())
        } else {
            Err(invalid("Maps need Parhelion's D2 model importer"))
        }
    }

    pub(crate) fn len(&self) -> usize {
        0
    }

    pub(crate) fn parts(&self, _index: usize) -> Vec<Part> {
        Vec::new()
    }

    pub(crate) fn package_count(&self) -> usize {
        0
    }

    pub(crate) fn family(&self, _index: usize, _part: Part) -> String {
        String::new()
    }

    pub(crate) fn registration(
        &self,
        _id: u16,
        _tags: &[crate::NewTagSpec],
        _registered: &std::collections::BTreeSet<u16>,
    ) -> AuthoringResult<Vec<TagHash>> {
        Ok(Vec::new())
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn package(
        &self,
        _index: usize,
        _part: Part,
        _id: u16,
    ) -> AuthoringResult<(
        Vec<NewTagSpec>,
        Vec<NamedTag>,
        Vec<crate::extend::NewTagReferenceOverride>,
    )> {
        Ok(Default::default())
    }

    pub(crate) fn strings(&self) -> Vec<(u32, String)> {
        Vec::new()
    }

    pub(crate) fn is_empty(&self) -> bool {
        true
    }

    pub(crate) fn pictures(&self) -> &[super::Picture] {
        &[]
    }

    pub(crate) fn set_pictures(&mut self, _rows: &[(usize, u16)]) -> AuthoringResult<()> {
        Ok(())
    }
}
