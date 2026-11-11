//! Native sphere queries with an outgoing-profile action, retargeted to a source radial falloff,
//! and the private copies of native effect entities that hold them.
use crate::{
    halo_reach::projectile,
    presentation::put,
    tiger::{entity::compiled, payload::Payload, projectile::Assets, reader::Reader},
};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

/// A native effect entity being copied into private assets: every component owner and
/// allocation gets a private identity, and `converted` holds the owners to edit before
/// [`Copy::finish`] relocates and emits them.
pub(super) struct Copy {
    carrier: u32,
    prefix: String,
    original: Payload,
    rows: Vec<usize>,
    owners: BTreeMap<u32, Payload>,
    ids: BTreeMap<u32, u32>,
    allocations: BTreeMap<u32, u32>,
    replicated: bool,
    pub converted: BTreeMap<u32, Payload>,
}

impl Copy {
    pub fn begin(
        native: &mut Reader,
        assets: &mut Assets,
        carrier: u32,
        prefix: &str,
    ) -> Result<Self> {
        Self::open(native, assets, carrier, prefix, true)
    }

    /// As [`Copy::begin`], for a graph an unreplicated projectile creates. The copy names no
    /// replication record and leaves out the network component that would read one, as
    /// [`projectile::replication::strip`] describes.
    pub fn local(
        native: &mut Reader,
        assets: &mut Assets,
        carrier: u32,
        prefix: &str,
    ) -> Result<Self> {
        Self::open(native, assets, carrier, prefix, false)
    }

    fn open(
        native: &mut Reader,
        assets: &mut Assets,
        carrier: u32,
        prefix: &str,
        replicated: bool,
    ) -> Result<Self> {
        let mut original = (*native.tag(carrier, Some(0x80809c0f))?).clone();
        if !replicated {
            let mut stock = BTreeMap::new();
            for row in original.array(16, 12, Some(0x80809c04))? {
                let tag = original.u32(row)?;
                stock.insert(tag, (*native.tag(tag, Some(0x80809c36))?).clone());
            }
            original = projectile::replication::strip(native, &original, &stock)?.0;
        }
        let rows = original.array(16, 12, Some(0x80809c04))?;
        let mut owners = BTreeMap::new();
        let mut ids = BTreeMap::new();
        let mut allocations = BTreeMap::new();
        for &row in &rows {
            let tag = original.u32(row)?;
            let p = native.tag(tag, Some(0x80809c36))?;
            ensure!(
                owners.insert(tag, (*p).clone()).is_none(),
                "Native effect repeats a component"
            );
            ids.insert(
                tag,
                assets.reserve(native, format!("{prefix}-component-{tag:08X}"))?,
            );
            let allocation = p.u32(0x44)?;
            if let std::collections::btree_map::Entry::Vacant(slot) = allocations.entry(allocation)
            {
                slot.insert(
                    assets.reserve(native, format!("{prefix}-allocation-{allocation:08X}"))?,
                );
            }
        }
        Ok(Self {
            carrier,
            prefix: prefix.to_owned(),
            original,
            rows,
            converted: owners.clone(),
            owners,
            ids,
            allocations,
            replicated,
        })
    }

    /// Emits the edited owners, the allocations, a rebuilt replication unless the copy is
    /// local, and the entity, and returns the private root identity, reserved under `root`.
    pub fn finish(self, native: &mut Reader, assets: &mut Assets, root: &str) -> Result<u32> {
        let mut converted = self.converted;
        for (&tag, p) in &mut converted {
            projectile::relocate(p, &self.owners, &self.ids, &[])?;
            let allocation = self.allocations[&self.owners[&tag].u32(0x44)?];
            put(&mut p.0, 0x44, &allocation.to_le_bytes())?;
            assets.push(self.ids[&tag], tag, p.clone(), None)?;
        }
        for (&tag, &id) in &self.allocations {
            assets.push(id, tag, (*native.tag(tag, Some(0x80809bbb))?).clone(), None)?;
        }
        let root_id = assets.reserve(native, root.into())?;
        let mut entity = self.original.clone();
        projectile::relocate(&mut entity, &self.owners, &self.ids, &self.rows)?;
        if self.replicated {
            let replication = assets.reserve(native, format!("{}-replication", self.prefix))?;
            let components = self
                .rows
                .iter()
                .map(|&row| {
                    let tag = self.original.u32(row)?;
                    Ok(compiled::Component {
                        owner: self.ids[&tag],
                        payload: &converted[&tag],
                        template: &self.owners[&tag],
                        entity: &self.original,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let replication_template = self.original.u32(0x88)?;
            native.tag(replication_template, Some(0x80809bb6))?;
            assets.push(
                replication,
                replication_template,
                compiled::replication::emit(root_id, &components)?,
                None,
            )?;
            put(&mut entity.0, 0x88, &replication.to_le_bytes())?;
        } else {
            ensure!(
                entity.u32(0x88)? == u32::MAX,
                "A local effect copy names a replication record"
            );
        }
        assets.push(root_id, self.carrier, entity, None)?;
        Ok(root_id)
    }
}

/// The checked plateau-linear spatial program: weight one through the falloff start, then
/// linear to zero at the sphere's edge.
const PLATEAU_LINEAR: [u8; 14] = [
    0x3c, 0, 0x22, 0, 0x23, 0x22, 0, 0x37, 0, 0x23, 0x22, 0, 0x3e, 0,
];

/// The sphere shape of `80804AB9` query `q`, after checking its weight policy.
pub(super) fn sphere(p: &Payload, q: usize) -> Result<usize> {
    ensure!(
        p.u8(q + 0x30)? == 0x1d
            && p.u32(q + 0xa0)? == 0
            && p.u64(q + 0xd0)? == 1
            && p.u64(q + 0xd8)? == 1
            && p.u64(q + 0x160)? == 0,
        "Native query uses another weight policy"
    );
    let shape = p.pointer(q + 0x48)?;
    ensure!(
        p.u32(shape - 4)? == 0x808092b1 && p.u32(shape)? == 2,
        "Native query requires a sphere"
    );
    Ok(shape)
}

/// The typed outgoing-profile action of query `q`, which must have exactly one action.
pub(super) fn action(p: &Payload, q: usize) -> Result<usize> {
    let actions = p.array(q + 0x178, 0x58, Some(0x80804abf))?;
    let [action] = actions.as_slice() else {
        anyhow::bail!("Native query needs one outgoing action")
    };
    let action = p.pointer(action + 0x50)?;
    ensure!(
        p.u32(action - 4)? == 0x80804ac9,
        "Native query uses another action"
    );
    Ok(action)
}

/// Writes the falloff start into the query's spatial program and points its one action at
/// `profile`, as [`falloff`] describes.
pub(super) fn retarget(
    p: &mut Payload,
    q: usize,
    start: f32,
    radius: Option<f32>,
    profile: u32,
) -> Result<()> {
    falloff(p, q, start, radius)?;
    let action = action(p, q)?;
    put(&mut p.0, action + 8, &profile.to_le_bytes())?;
    Ok(())
}

/// Writes the falloff start, as a fraction of the radius, into the query's spatial program. A
/// `radius` replaces the sphere radius, else the native radius stays and any pose scale the
/// graph applies to it still applies.
pub(super) fn falloff(p: &mut Payload, q: usize, start: f32, radius: Option<f32>) -> Result<()> {
    let shape = sphere(p, q)?;
    if let Some(radius) = radius {
        ensure!(
            radius.is_finite() && radius > 0.,
            "Invalid translated query radius"
        );
        put(&mut p.0, shape + 4, &radius.to_le_bytes())?;
    }
    let code = p.array(q + 0xb0, 1, Some(0x80800009))?;
    ensure!(
        code.len() == PLATEAU_LINEAR.len()
            && code
                .iter()
                .zip(PLATEAU_LINEAR)
                .all(|(&at, byte)| p.0[at] == byte),
        "Native query has another spatial program"
    );
    let constants = p.array(q + 0xc0, 16, Some(0x80800090))?;
    ensure!(
        constants.len() == 5,
        "Native query has another spline layout"
    );
    let slope = 1. / (1. - start);
    ensure!(
        (0. ..1.).contains(&start) && slope.is_finite(),
        "Query falloff interval is too narrow"
    );
    // The source inner and outer radii with a linear attenuation between them. A Reach damage
    // caller's exponent mapping stays in the receipt until its recipient input is joined.
    let values = [
        [0.; 4],
        [0.; 4],
        [0., -slope, 0., 0.],
        [1., slope, 0., 0.],
        [0., start, 1., 1.],
    ];
    for (&at, row) in constants.iter().zip(values) {
        for (lane, value) in row.into_iter().enumerate() {
            put(&mut p.0, at + lane * 4, &value.to_le_bytes())?
        }
    }
    Ok(())
}
