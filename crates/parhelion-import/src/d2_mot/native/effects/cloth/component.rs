//! Compose native cloth callbacks and share the translated material channels.
use super::*;
use crate::d2_mot::markers::bindings;

fn copy_events(
    entity: &Payload,
    patches: &[Value],
    selected: impl Fn(usize) -> Result<bool>,
) -> Result<(Vec<u8>, Vec<Value>)> {
    let mut bytes = Vec::new();
    let mut refs = Vec::new();
    for at in entity.array(0x20, 72, Some(0x80809BC9))? {
        if !selected(at)? {
            continue;
        }
        for patch in patches {
            let offset = number(&patch["offset"])?;
            if (at..at + 72).contains(&offset) {
                refs.push(json!({"offset":bytes.len()+offset-at,"symbol":patch["symbol"]}));
            }
        }
        bytes.extend_from_slice(&entity.0[at..at + 72]);
    }
    Ok((bytes, refs))
}

fn input(entity: &Payload, refs: &[Value], at: usize) -> Result<bool> {
    Ok(entity.u32(at + 12)? == 0x80809789
        && refs
            .iter()
            .any(|p| p["offset"].as_u64() == Some((at + 8) as u64) && p["symbol"] == "owner"))
}

pub(super) fn build(
    c: &mut Effect,
    reader: &mut Reader,
    name: &str,
    template: &Payload,
) -> Result<()> {
    let symbol = format!("{name}-owner");
    let mut owner = template.clone();
    let instance = owner.pointer(16)?;
    let resource = owner.pointer(24)?;
    let parent = instance + 0x100;
    let schema_inputs = usize::try_from(owner.u64(parent + 8)?)?
        .checked_add(0x48)
        .context("Cloth input schema overflow")?;
    let old_inputs = owner.array(instance + 0x120, 96, Some(0x80809788))?;
    let model_owner = c.graph.read("owner")?;
    let model_instance = model_owner.pointer(16)?;
    let model_schema = usize::try_from(model_owner.u64(model_instance + 0x108)?)?
        .checked_add(0x48)
        .context("Model input schema overflow")?;
    let model_inputs = model_owner.array(model_instance + 0x120, 96, Some(0x80809788))?;
    let model_links = model_owner.array(model_schema, 40, Some(0x80809789))?;
    ensure!(
        !model_inputs.is_empty()
            && model_inputs.len() == model_links.len()
            && model_inputs.len() == c.objects.len(),
        "Cloth material input extent differs"
    );
    let mut owner_patches = Vec::new();
    for at in (0..owner.0.len().saturating_sub(15)).step_by(4) {
        if owner.u32(at)? == OWNER {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "Untyped cloth owner self-reference"
            );
            put(&mut owner.0, at, &u32::MAX.to_le_bytes())?;
            patch(&mut owner_patches, at, &symbol);
        }
    }
    let input_data = model_inputs
        .iter()
        .flat_map(|at| model_owner.0[*at..*at + 96].iter().copied())
        .collect::<Vec<_>>();
    let link_data = model_links
        .iter()
        .flat_map(|at| model_owner.0[*at..*at + 40].iter().copied())
        .collect::<Vec<_>>();
    append_array(&mut owner.0, instance + 0x120, 0x80809788, &input_data, 96)?;
    append_array(&mut owner.0, schema_inputs, 0x80809789, &link_data, 40)?;
    let inputs = owner.array(instance + 0x120, 96, Some(0x80809788))?;
    let links = owner.array(schema_inputs, 40, Some(0x80809789))?;
    let mut link_map = BTreeMap::new();
    for ((input, link), old) in inputs.into_iter().zip(&links).zip(&model_links) {
        let link = *link;
        put(&mut owner.0, input, &u32::MAX.to_le_bytes())?;
        put(&mut owner.0, input + 8, &(link as u64).to_le_bytes())?;
        put(
            &mut owner.0,
            input + 16,
            &(parent as i64 - (input + 16) as i64).to_le_bytes(),
        )?;
        put(&mut owner.0, link, &u32::MAX.to_le_bytes())?;
        put(&mut owner.0, link + 8, &(input as u64).to_le_bytes())?;
        patch(&mut owner_patches, input, &symbol);
        patch(&mut owner_patches, link, &symbol);
        link_map.insert(*old as u64, link as u64);
    }
    let allocation_tag = owner.u32(0x44)?;
    let mut allocation = (*reader.tag(allocation_tag, None)?).clone();
    ensure!(
        channels::input_allocation(&mut allocation, 0x20, old_inputs.len(), links.len())? == 1,
        "Cloth input allocation is absent or ambiguous"
    );
    let allocation_symbol = format!("{name}-input-allocation");
    c.graph.add(
        &allocation_symbol,
        u64::from(allocation_tag),
        &allocation.0,
        None,
        vec![],
    )?;
    for (at, target) in [
        (0x44, allocation_symbol),
        (resource + 0x1DC, format!("{name}-model")),
        (resource + 0x248, "plates".to_owned()),
        (resource + 0x358, format!("{name}-definition")),
    ] {
        put(&mut owner.0, at, &u32::MAX.to_le_bytes())?;
        patch(&mut owner_patches, at, &target);
    }
    let end = owner.0.len();
    layout::seal_span(&mut owner, &mut owner_patches, end)?;
    c.graph
        .add(&symbol, u64::from(OWNER), &owner.0, None, owner_patches)?;

    let mut entity = c.graph.read("entity")?;
    let mut entity_patches = patches(&c.graph, "entity")?;
    let mut template_entity = (*reader.tag(ENTITY, Some(0x80809C0F))?).clone();
    // The native rig endpoint belongs to the component. Its old vector inputs
    // are replaced with links to this import's compiled channel bank below.
    let (events, _) = copy_events(&template_entity, &[], |at| {
        Ok(![8, 40].into_iter().any(|delta| {
            template_entity.u32(at + delta).ok() == Some(OWNER)
                && template_entity.u32(at + delta + 4).ok() == Some(0x80809789)
        }))
    })?;
    bindings::append(
        &mut template_entity,
        &mut vec![],
        0x20,
        72,
        0x80809BC9,
        &events,
        &[],
    )?;
    bindings::graft::insert(
        reader,
        &mut entity,
        &mut entity_patches,
        &template_entity,
        &BTreeMap::from([(OWNER, symbol.clone())]),
        &BTreeMap::new(),
    )?;
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let component_index = components
        .iter()
        .position(|at| {
            entity_patches
                .iter()
                .any(|p| p["offset"].as_u64() == Some(*at as u64) && p["symbol"] == symbol)
        })
        .context("Cloth component registration is absent")?;
    let (mut events, mut event_refs) = copy_events(&entity, &entity_patches, |_| Ok(true))?;
    let mut connected = std::collections::BTreeSet::new();
    for at in entity.array(0x20, 72, Some(0x80809BC9))? {
        if !input(&entity, &entity_patches, at)? {
            continue;
        }
        let old = entity.u64(at + 16)?;
        let new = *link_map
            .get(&old)
            .context("Cloth material connection addresses an unknown input")?;
        ensure!(
            connected.insert(old),
            "Model vector input has multiple connections"
        );
        let start = events.len();
        let mut row = entity.0[at..at + 72].to_vec();
        put(&mut row, 8, &u32::MAX.to_le_bytes())?;
        put(&mut row, 16, &new.to_le_bytes())?;
        put(&mut row, 24, &u32::try_from(component_index)?.to_le_bytes())?;
        for p in &entity_patches {
            let offset = number(&p["offset"])?;
            if (at..at + 72).contains(&offset) && offset != at + 8 {
                event_refs.push(json!({"offset":start+offset-at,"symbol":p["symbol"]}));
            }
        }
        event_refs.push(json!({"offset":start+8,"symbol":symbol}));
        events.extend(row);
    }
    ensure!(
        connected.len() == links.len(),
        "Cloth material channels are not fully connected"
    );
    bindings::append(
        &mut entity,
        &mut entity_patches,
        0x20,
        72,
        0x80809BC9,
        &events,
        &event_refs,
    )?;
    c.graph.write("entity", &entity.0)?;
    c.graph.node_mut("entity")?["patches"] = json!(entity_patches);
    Ok(())
}
