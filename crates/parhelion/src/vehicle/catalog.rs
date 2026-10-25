//! Vehicle picker candidates from native component bindings. Builds still validate live graphs.
use sundial::package_authoring::sandbox_perk::entity::catalog::{Catalog, Entry};

#[derive(Clone, Copy)]
pub(crate) struct Capabilities {
    pub hover: bool,
    pub armed: bool,
}

pub(crate) fn capabilities(catalog: &Catalog, entry: &Entry) -> Option<Capabilities> {
    if entry.object_type != 15 {
        return None;
    }
    let mut markers = false;
    let mut hover = false;
    let mut tank = false;
    let mut armed = false;
    for component in entry
        .owners
        .iter()
        .filter_map(|owner| catalog.owners.get(owner))
        .flat_map(|owner| &owner.components)
    {
        match component.class {
            0x8080_42AD => markers = true,
            0x8080_3CDE => hover = true,
            0x8080_3CAA => tank = true,
            0x8080_9425 => armed = true,
            _ => {}
        }
    }
    (markers && (hover || tank)).then_some(Capabilities { hover, armed })
}

pub(crate) fn moving_projectile(catalog: &Catalog, entry: &Entry) -> bool {
    entry.object_type == 18
        && entry
            .owners
            .iter()
            .filter_map(|owner| catalog.owners.get(owner))
            .flat_map(|owner| &owner.components)
            .any(|component| component.class == 0x8080_3B73)
}
