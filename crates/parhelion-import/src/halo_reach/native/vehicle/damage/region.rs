//! Damage states publish the same native condition pairs used by model groups.
use super::*;

const REGION: u32 = 0x51e7a18d;
const DAMAGE: u32 = 0x6a807a1a;
pub(super) const STATES: [u32; 5] = [0x2ca33bdb, 0xd09ad130, 0xa9d28bb0, 0x4a3554b4, 0x51033a3a];

pub(in super::super) fn parts(
    payload: &mut Payload,
    definition: usize,
    groups: &[(String, Vec<u8>)],
) -> Result<()> {
    ensure!(
        groups.len() < 512,
        "Vehicle part conditions exceed the native change bitmap"
    );
    let rows = array(
        payload,
        definition + 0x38,
        0x80809c2c,
        0x18,
        groups.len() + 1,
    )?;
    for &row in &rows {
        put(&mut payload.0, row, &0x18u32.to_le_bytes())?;
    }
    for (&row, (name, states)) in rows[1..].iter().zip(groups) {
        ensure!(
            !name.is_empty() && !states.is_empty(),
            "Empty vehicle part condition"
        );
        let pairs = array(payload, row + 8, 0x80809c31, 8, states.len() + 1)?;
        put(&mut payload.0, pairs[0], &REGION.to_le_bytes())?;
        put(&mut payload.0, pairs[0] + 4, &hash(name).to_le_bytes())?;
        for (&pair, &ordinal) in pairs[1..].iter().zip(states) {
            let state = *STATES
                .get(usize::from(ordinal))
                .context("Vehicle damage state ordinal")?;
            put(&mut payload.0, pair, &DAMAGE.to_le_bytes())?;
            put(&mut payload.0, pair + 4, &state.to_le_bytes())?;
        }
    }
    Ok(())
}

pub(super) fn health(
    payload: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    geometry: &Geometry,
) -> Result<Value> {
    let definitions = array(
        payload,
        definition + 0x378,
        0x80804b99,
        0x28,
        geometry.regions.len(),
    )?;
    let instances = array(
        payload,
        instance + 0x378,
        0x80804b98,
        0x30,
        geometry.regions.len(),
    )?;
    let mut report = Vec::new();
    for (index, ((region, &d), &i)) in geometry
        .regions
        .iter()
        .zip(&definitions)
        .zip(&instances)
        .enumerate()
    {
        reciprocal(payload, owner, i, d, 0x80804b98, 0x80804b99, instance)?;
        let name = hash(&region.name);
        ensure!(name != 0x811c9dc5, "Source damage region is unnamed");
        put(&mut payload.0, d + 16, &name.to_le_bytes())?;
        let states = array(payload, d + 24, 0x80804b9b, 0x3c, STATES.len())?;
        for (ordinal, (&at, state)) in states.iter().zip(STATES).enumerate() {
            for offset in (4..0x1c).step_by(4).chain((0x20..0x38).step_by(4)) {
                put(&mut payload.0, at + offset, &0x811c9dc5u32.to_le_bytes())?;
            }
            put(&mut payload.0, at, &1u32.to_le_bytes())?;
            put(&mut payload.0, at + 4, &REGION.to_le_bytes())?;
            put(&mut payload.0, at + 8, &name.to_le_bytes())?;
            put(&mut payload.0, at + 0x1c, &1u32.to_le_bytes())?;
            put(&mut payload.0, at + 0x20, &DAMAGE.to_le_bytes())?;
            put(&mut payload.0, at + 0x24, &state.to_le_bytes())?;
            put(
                &mut payload.0,
                at + 0x38,
                &u32::try_from(ordinal)?.to_le_bytes(),
            )?;
        }
        report.push(json!({"name":region.name,"index":index,"instance":i,"definition":d,"permutations":region.states}));
    }
    Ok(json!(report))
}

pub(super) fn render(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    geometry: &Geometry,
) -> Result<Value> {
    let owner = assembly.owner(0x808072b8)?;
    let (allocation_tag, mut layout) = assembly.allocation(native, graph, owner)?;
    let model = graph.read("model")?;
    let capacity = usize::try_from(model.u32(crate::tiger::draws::DRAW_INDEX_CAPACITY)?)?;
    ensure!(
        capacity > 0 && capacity.is_multiple_of(32),
        "Vehicle model draw capacity differs"
    );
    let words = capacity / 32;
    let payload = assembly.payloads.get_mut(&owner).unwrap();
    let (instance, definition) = definition(payload, 0x808072b8, 0x808072bd)?;
    let groups = geometry
        .groups
        .iter()
        .map(|group| {
            (
                geometry.regions[group.region].name.clone(),
                group.states.clone(),
            )
        })
        .collect::<Vec<_>>();
    parts(payload, definition, &groups)?;
    let mut report = Vec::new();
    // Group zero is deliberately authored as unconditional for model attachments.
    // It has no special meaning in the native format.
    for (index, group) in geometry.groups.iter().enumerate() {
        let region = &geometry.regions[group.region];
        report.push(json!({"group":index+1,"region":region.name,"permutation":group.permutation,"states":group.states}));
    }
    array(payload, instance + 0x170, 0x8080000b, 4, words)?;
    let end = payload.0.len();
    allocation(&mut layout, &[0x7da384e0], 0x8080000b, words)?;
    assembly.runtime_end(owner, end)?;
    assembly.set_allocation(graph, owner, allocation_tag, &layout)?;
    Ok(
        json!({"groups":report,"draw_capacity":capacity,"bitmap_words":words,"owner":format!("{owner:08X}")}),
    )
}
