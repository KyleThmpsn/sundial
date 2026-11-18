//! Persisted magazine-reload composition with an independently chosen ordinary firing donor.
use super::*;
use sundial::package_authoring::entity::{
    WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY,
    WEAPON_RELOAD_COMPONENT_KEY, WEAPON_TRIGGER_COMPONENT_KEY, barrel, barrel_pellets,
};

fn binding(
    entity: &[u8],
    key: u32,
) -> Result<sundial::package_authoring::entity::WeaponComponentBinding> {
    let rows = weapon_component_bindings(entity, key).map_err(anyhow::Error::msg)?;
    let [row] = rows.as_slice() else {
        anyhow::bail!("Ambiguous staged component {key:08X}")
    };
    Ok(*row)
}

fn firing_graph(reader: &mut Reader, entity: &[u8], content: u32) -> Result<Option<u32>> {
    let binding = binding(entity, 0x5F0DD954)?;
    let owner = reader.tag(binding.owner_tag, Some(0x80809C36))?;
    let base = usize::try_from(owner.u64(binding.resource_offset as usize + 8)?)?;
    let mut selected = base + 0x80;
    for row in owner.array(base + 0x240, 0x1C0, None)? {
        if owner.u32(row + 16)? == content {
            selected = row;
            break;
        }
    }
    let graph = owner.u32(selected + 0xF0)?;
    Ok((graph != 0 && graph != u32::MAX).then_some(graph))
}

fn firing_definition(
    manager: &sundial::package_authoring::PackageManager,
    template: u32,
    payload: &Payload,
    root: usize,
    size: usize,
    owner: u32,
) -> Result<Vec<u8>> {
    let mut bytes = payload
        .0
        .get(root + 16..root + size)
        .context("Firing definition bounds")?
        .to_vec();
    // Normalize only typed self references. Scalar firing settings and external resources remain.
    for (field, target) in sundial::package_authoring::declared_payload_object_references(
        manager, template, &payload.0,
    )
    .map_err(anyhow::Error::msg)?
    {
        if target != owner || field < root + 16 || field + 16 > root + size {
            continue;
        }
        bytes[field - root - 16..field - root - 12].fill(0);
        let class = match payload.u32(field + 4)? {
            0x80BFDFEA => 0x80C1C534u32,
            0x80BFDFEB => 0x80C1C533u32,
            class => class,
        };
        bytes[field - root - 12..field - root - 8].copy_from_slice(&class.to_le_bytes());
    }
    Ok(bytes)
}

fn firing(
    manager: &sundial::package_authoring::PackageManager,
    (entity, owner): (&[u8], &Payload),
    (donor_entity, donor): (&[u8], &Payload),
) -> Result<Value> {
    let controller = binding(entity, WEAPON_CONTROLLER_COMPONENT_KEY)?;
    let stock_controller = binding(donor_entity, WEAPON_CONTROLLER_COMPONENT_KEY)?;
    let mut firing = Vec::new();
    for (key, size) in [
        (WEAPON_TRIGGER_COMPONENT_KEY, 0x88usize),
        (WEAPON_BARREL_COMPONENT_KEY, 0x1DC0),
    ] {
        let actual = binding(entity, key)?;
        let stock = binding(donor_entity, key)?;
        ensure!(
            actual.concrete_class == stock.concrete_class
                && actual.resource_offset == stock.resource_offset,
            "The ordinary firing component changed shape"
        );
        let at = usize::try_from(owner.u64(actual.resource_offset as usize + 8)?)?;
        let stock_at = usize::try_from(donor.u64(stock.resource_offset as usize + 8)?)?;
        let got = firing_definition(
            manager,
            stock_controller.owner_tag,
            owner,
            at,
            size,
            controller.owner_tag,
        )?;
        let want = firing_definition(
            manager,
            stock_controller.owner_tag,
            donor,
            stock_at,
            size,
            stock_controller.owner_tag,
        )?;
        ensure!(
            got == want,
            "Magazine adaptation changed ordinary firing definition {key:08X}"
        );
        firing.push(
            json!({"binding":key,"class":actual.concrete_class,"definition_sha256":digest(&got)}),
        );
    }
    let actual = binding(entity, WEAPON_BARREL_COMPONENT_KEY)?;
    let stock = binding(donor_entity, WEAPON_BARREL_COMPONENT_KEY)?;
    let shots = barrel::projectile_slots(&owner.0, actual).map_err(anyhow::Error::msg)?;
    ensure!(
        shots == barrel::projectile_slots(&donor.0, stock).map_err(anyhow::Error::msg)?
            && barrel_pellets(&owner.0, actual).map_err(anyhow::Error::msg)?
                == barrel_pellets(&donor.0, stock).map_err(anyhow::Error::msg)?,
        "Magazine adaptation changed the ordinary shotgun's shot"
    );
    Ok(json!({"components":firing,"shots":shots}))
}

fn magazine_inputs(
    reader: &mut Reader,
    entity: &[u8],
    translation: u32,
    (capacity, refill): (f64, f64),
) -> Result<Value> {
    let translator = binding(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)?;
    let translator = reader.tag(translator.owner_tag, Some(0x80809C36))?;
    let base = translator.pointer(24)?;
    let groups = translator.array(base + 0x48, 48, Some(0x80803975))?;
    let keys = translator.array(base + 0x238, 40, Some(0x808038C7))?;
    let index = keys
        .iter()
        .position(|&r| translator.u32(r + 16).ok() == Some(translation))
        .unwrap_or(0);
    let routes = translator.array(groups[index] + 16, 80, Some(0x80803981))?;
    let mut values = BTreeMap::new();
    for input in [0, 3] {
        let rows = routes
            .iter()
            .copied()
            .filter(|&r| {
                translator.u32(r).ok() == Some(1) && translator.u32(r + 4).ok() == Some(input)
            })
            .collect::<Vec<_>>();
        ensure!(!rows.is_empty(), "Magazine input has no writer");
        let mut read = Vec::new();
        for row in rows {
            ensure!(
                translator.u32(row + 12)? == 4 && translator.bytes::<16>(row + 16)? == [0; 16],
                "Magazine input is not the expected direct literal"
            );
            read.push(translator.f32(row + 32)?);
        }
        ensure!(
            read.iter()
                .all(|&v| f64::from(v) == if input == 0 { capacity } else { refill }),
            "Magazine capacity or refill quantum differs from the required mechanism"
        );
        values.insert(input, read);
    }
    Ok(json!(values))
}

/// The engine dispatches through this resource, independently of the object's class words.
fn reload_methods(reader: &mut Reader, owner: &Payload, reload: usize) -> Result<(u32, Vec<u8>)> {
    let definition = usize::try_from(owner.u64(reload + 8)?)?;
    let metadata = owner.u32(definition + 0xB8)?;
    let table = reader.tag(metadata, Some(0x80809C54))?;
    ensure!(
        table.u32(8)? == 0x80803AFA && table.u32(12)? == 0x80803AF9,
        "Reload publisher has another interface"
    );
    let rows = table.array(16, 24, Some(0x80809C56))?;
    ensure!(!rows.is_empty(), "Reload method table is empty");
    Ok((
        metadata,
        rows.into_iter()
            .flat_map(|at| table.0[at..at + 8].to_vec())
            .collect(),
    ))
}

pub(super) fn readback(
    readers: (
        &mut Reader,
        &mut Reader,
        &sundial::package_authoring::PackageManager,
    ),
    runtime: (&[u8], u32, u32),
    donor: (&[u8], u32),
    lookup: (&Payload, usize),
    expected: &Value,
    output: &Path,
) -> Result<Value> {
    if expected.is_null() {
        return Ok(Value::Null);
    }
    let (reader, original, manager) = readers;
    let (entity, translation, content) = runtime;
    let (donor_entity, donor_content) = donor;
    let (lookup, base) = lookup;
    let controller = binding(entity, WEAPON_CONTROLLER_COMPONENT_KEY)?;
    let reload = binding(entity, WEAPON_RELOAD_COMPONENT_KEY)?;
    let magazine = binding(entity, WEAPON_MAGAZINE_COMPONENT_KEY)?;
    let capacity = expected["capacity"].as_f64().context("Required capacity")?;
    let refill = expected["refill"]
        .as_f64()
        .context("Required refill quantum")?;
    if expected["mode"] == "shell" {
        ensure!(
            controller.concrete_class == 0x80C1C534 && reload.concrete_class == 0x80803842,
            "Per-shell reload changed its native mechanism"
        );
        let stock = binding(donor_entity, WEAPON_RELOAD_COMPONENT_KEY)?;
        let got = reader.tag(reload.owner_tag, Some(0x80809C36))?;
        let want = original.tag(stock.owner_tag, Some(0x80809C36))?;
        ensure!(
            reload_methods(reader, &got, reload.resource_offset as usize)?.1
                == reload_methods(original, &want, stock.resource_offset as usize)?.1,
            "Per-shell reload changed its native method dispatch"
        );
        let values = magazine_inputs(reader, entity, translation, (capacity, refill))?;
        return Ok(
            json!({"mode":"shell","controller":controller.owner_tag,"magazine_inputs":values}),
        );
    }
    ensure!(
        expected["mode"] == "magazine",
        "Unknown reload acceptance mode"
    );
    ensure!(
        controller.concrete_class == 0x80BFDFEA && reload.concrete_class == 0x80803844,
        "Magazine reload still uses the shell controller"
    );
    ensure!(
        controller.owner_tag == reload.owner_tag && controller.owner_tag == magazine.owner_tag,
        "Magazine reload detached from the firing controller"
    );
    let owner = reader.tag(controller.owner_tag, Some(0x80809C36))?;
    let definition = usize::try_from(owner.u64(controller.resource_offset as usize + 8)?)?;
    ensure!(
        owner.u32(definition - 4)? == 0x80BFDFEB && owner.u32(definition + 4)? == 0x80BFDFEA,
        "Controller's reciprocal definition has the wrong composition"
    );
    let reload_definition = usize::try_from(owner.u64(reload.resource_offset as usize + 8)?)?;
    ensure!(
        owner.u32(reload.resource_offset as usize + 4)? == 0x80803845
            && owner.u32(reload_definition + 4)? == 0x80803844,
        "Reload's reciprocal definition has the wrong composition"
    );
    let (metadata, methods) = reload_methods(reader, &owner, reload.resource_offset as usize)?;
    // This stock magazine controller is an independent native oracle, not the conversion's
    // class map. Follow its actual publisher rather than assuming an interface resource tag.
    let native_magazine = original.tag(0x81522686, Some(0x80809C36))?;
    let (_, expected_methods) = reload_methods(original, &native_magazine, 0x2790)?;
    ensure!(
        methods == expected_methods,
        "Magazine reload still dispatches shell overrides"
    );
    let stock_controller = binding(donor_entity, WEAPON_CONTROLLER_COMPONENT_KEY)?;
    ensure!(
        controller.owner_tag != stock_controller.owner_tag,
        "Reload mutated a stock controller"
    );
    let donor = original.tag(stock_controller.owner_tag, Some(0x80809C36))?;
    let firing = firing(manager, (entity, &owner), (donor_entity, &donor))?;
    let graph = firing_graph(reader, entity, content)?;
    ensure!(
        graph == firing_graph(original, donor_entity, donor_content)?,
        "Selected content replaced the ordinary shotgun's firing graph"
    );
    let states = reader.tag(lookup.u32(base + 0x9C)?, Some(0x80803465))?;
    let mut actions = Vec::new();
    for name in ["reload_full", "reload_empty"] {
        let hash = name.bytes().fold(0x811C9DC5u32, |h, byte| {
            h.wrapping_mul(0x01000193) ^ u32::from(byte)
        });
        let descriptors = parhelion_import::tiger::animation::state::descriptors(&states, hash)?;
        ensure!(
            !descriptors.is_empty(),
            "Magazine reload action is unreachable"
        );
        actions.push(json!({"action":name,"descriptors":descriptors}));
    }
    let values = magazine_inputs(reader, entity, translation, (capacity, refill))?;
    fs::write(output.join("magazine-controller.bin"), &owner.0)?;
    fs::write(output.join("ordinary-controller.bin"), &donor.0)?;
    Ok(
        json!({"controller":controller.owner_tag,"sha256":digest(&owner.0),"stock_owner":stock_controller.owner_tag,
        "stock_sha256":digest(&donor.0),"firing":firing,"content_graph":graph,"actions":actions,"magazine_inputs":values,
        "reload_methods":{"resource":metadata,"class_and_index_sha256":digest(&methods)}}),
    )
}
