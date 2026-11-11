//! Preserve selected content, default content and the two Barrel launch slots in that order.
use super::*;
use sundial::package_authoring::entity::{WEAPON_BARREL_COMPONENT_KEY, barrel::projectile_slots};

pub(super) struct SourceSelection {
    pub key: u32,
    pub graph: u32,
    pub route: &'static str,
}

fn source_barrel(source: &Source, entity: &Payload) -> Result<u32> {
    let owners = entity
        .array(32, 12, Some(0x80800715))?
        .into_iter()
        .map(|r| entity.u32(r))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut slots = BTreeMap::new();
    for row in entity.array(0x68, 40, Some(0x80800370))? {
        if entity.u32(row + 12)? != 0x8080221A {
            continue;
        }
        let tag = entity.u32(row + 16)?;
        ensure!(
            owners.contains(&tag) && source.reference(tag)? == 0x80800861,
            "Source Barrel export has an external owner"
        );
        let owner = source.read(tag)?;
        let provider = usize::try_from(entity.u64(row + 24)?)?;
        let definition = owner.pointer(provider)?;
        let instance = usize::try_from(owner.u64(definition + 8)?)?;
        ensure!(
            owner.u32(definition)? == tag
                && owner.u64(definition + 16)? == 0x8080221A
                && owner.u32(instance)? == tag
                && owner.u64(instance + 16)? == 0x808022BD
                && owner.u64(instance + 8)? == definition as u64,
            "Source Barrel export has an invalid typed pair"
        );
        slots.insert(
            (tag, definition),
            [
                owner.u32(definition + 0x9B8)?,
                owner.u32(definition + 0x9C8)?,
            ],
        );
    }
    ensure!(slots.len() == 1, "Source has no unique Barrel launch slots");
    let pair = slots.into_values().next().context("Source Barrel slots")?;
    ensure!(
        pair[0] == pair[1] && !matches!(pair[0], 0 | u32::MAX),
        "Source Barrel launch modes select different or absent graphs"
    );
    Ok(pair[0])
}

pub(super) fn source(source: &Source, export: &Value) -> Result<SourceSelection> {
    let content = super::super::super::content_row(source, export)?;
    let key = content.key;
    let mut graph = content.owner.u32(content.row + 0x98)?;
    let mut route = if content.keyed {
        "content_variant"
    } else {
        "content_default"
    };
    if matches!(graph, 0 | u32::MAX) {
        graph = source_barrel(source, &content.entity)?;
        route = "barrel";
    }
    ensure!(
        source.reference(graph)? == 0x80800734,
        "Selected source projectile is not an entity"
    );
    Ok(SourceSelection { key, graph, route })
}

pub(super) struct Native {
    pub graph: u32,
    pub content: u32,
    pub resource: u64,
    pub key: u32,
    pub content_default: bool,
    pub barrel: Option<Value>,
    pub fingerprints: Vec<Value>,
}

fn native_barrel(native: &mut Reader, entity: &[u8]) -> Result<(u32, Value, Value)> {
    let bindings = weapon_component_bindings(entity, WEAPON_BARREL_COMPONENT_KEY)
        .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Native weapon has no unique Barrel owner");
    };
    let p = native.tag(binding.owner_tag, Some(0x80809C36))?;
    let slots = projectile_slots(&p.0, *binding).map_err(anyhow::Error::msg)?;
    ensure!(
        slots.len() == 2 && slots[0].1.is_some() && slots[0].1 == slots[1].1,
        "Native Barrel launch modes select different or absent graphs"
    );
    Ok((
        slots[0].1.context("Native Barrel projectile")?,
        json!({"owner":binding.owner_tag,"resource":binding.resource_offset}),
        fingerprint(binding.owner_tag, &p),
    ))
}

pub(super) fn native(native: &mut Reader, donor: u32, key: u32) -> Result<Native> {
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_weapon_runtime_entity_for_item_with_manager(&manager, donor)
        .map_err(anyhow::Error::msg)?;
    let bindings =
        weapon_component_bindings(&runtime.payload, CONTENT).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Native weapon has no unique content owner");
    };
    let p = native.tag(binding.owner_tag, Some(0x80809C36))?;
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == 0x80803AC9,
        "Native content definition class differs"
    );
    let selected = p
        .array(d + 0x240, 0x1C0, Some(0x80803ACF))?
        .into_iter()
        .filter(|&r| p.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() <= 1,
        "Native content has ambiguous source-key counterparts"
    );
    let row = selected.first().copied().unwrap_or(d + 0x80);
    let mut graph = p.u32(row + 0xF0)?;
    let mut fingerprints = vec![fingerprint(binding.owner_tag, &p)];
    let barrel = if matches!(graph, 0 | u32::MAX) {
        let (fallback, report, fingerprint) = native_barrel(native, &runtime.payload)?;
        graph = fallback;
        fingerprints.push(fingerprint);
        Some(report)
    } else {
        None
    };
    native.tag(graph, Some(0x80809C0F))?;
    Ok(Native {
        graph,
        content: binding.owner_tag,
        resource: binding.resource_offset,
        key: runtime.weapon_content_group_hash,
        content_default: selected.is_empty(),
        barrel,
        fingerprints,
    })
}
