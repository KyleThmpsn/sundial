//! Preserve selected content, default content and the two Barrel launch slots in that order. The
//! base's other content rows follow as carriers for a source projectile the first cannot take.
use super::*;
use sundial::package_authoring::entity::{WEAPON_BARREL_COMPONENT_KEY, barrel::projectile_slots};

#[derive(Clone, Copy)]
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
    /// The content row a later carrier comes from, as its offset from the content definition.
    /// The first carrier is the one the base itself selects and has none.
    pub row: Option<u64>,
}

/// The most carriers one source projectile is tried on.
const CARRIERS: usize = 12;

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

/// The carriers the base's content owner offers, each graph once. The first is the base's own
/// selection: the row under the source's content key, else the default row, else the Barrel
/// slots. Shadowkeep keeps rows under many Rise of Iron content keys, and such a row can name
/// another family's carrier, as the row a pulse rifle base keeps under an auto rifle source key
/// does. The default row and then every other row follow, so the source projectile can take a
/// carrier of its own family. Only the first carrier's errors are reported.
pub(super) fn carriers(native: &mut Reader, donor: u32, key: u32) -> Result<Vec<Native>> {
    let manager = sundial::package_authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let runtime = runtime::load_runtime_entity_for_item_with_manager(&manager, donor)
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
    let first = selected.first().copied().unwrap_or(d + 0x80);
    let mut order = vec![first];
    if first != d + 0x80 {
        order.push(d + 0x80);
    }
    order.extend(
        p.array(d + 0x240, 0x1C0, Some(0x80803ACF))?
            .into_iter()
            .filter(|&row| row != first),
    );
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for (index, row) in order.into_iter().enumerate() {
        let mut graph = p.u32(row + 0xF0)?;
        let mut fingerprints = vec![fingerprint(binding.owner_tag, &p)];
        let barrel = if matches!(graph, 0 | u32::MAX) {
            let slots = native_barrel(native, &runtime.payload);
            let (fallback, report, fingerprint) = match slots {
                Ok(slots) => slots,
                Err(error) if index == 0 => return Err(error),
                Err(_) => continue,
            };
            graph = fallback;
            fingerprints.push(fingerprint);
            Some(report)
        } else {
            None
        };
        if !seen.insert(graph) {
            continue;
        }
        if let Err(error) = native.tag(graph, Some(0x80809C0F)) {
            if index == 0 {
                return Err(error);
            }
            continue;
        }
        found.push(Native {
            graph,
            content: binding.owner_tag,
            resource: binding.resource_offset,
            key: runtime.weapon_content_group_hash,
            content_default: index == 0 && selected.is_empty(),
            barrel,
            fingerprints,
            row: (index > 0).then(|| (row - d) as u64),
        });
        if found.len() == CARRIERS {
            break;
        }
    }
    Ok(found)
}
