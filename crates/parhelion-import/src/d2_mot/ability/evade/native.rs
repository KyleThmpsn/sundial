//! A private native envelope for testing translated evade preparation math.
//!
//! Common ability energy and body animation remain explicit native adaptations. The source
//! controller's bank, effect graph and character selector are separate translation boundaries.
use super::{Direction, read, replace_motion};
use crate::{
    d2_mot::native::effects::controller,
    presentation::put,
    tiger::{
        allocation::Row,
        entity::{compiled, links::Graph},
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const ENTITY: u32 = 0x80B8024A;
const CONTROLLER: u32 = 0x80BC2C8B;
const MOVEMENT: u32 = 0x80BC3572;
const MOVEMENT_ENTITY: u32 = 0x80B81789;

fn size(p: &mut Payload) -> Result<()> {
    let length = p.0.len() as u64;
    put(&mut p.0, 0, &length.to_le_bytes())
}

fn pointer(p: &mut Payload, field: usize, target: usize) -> Result<()> {
    put(
        &mut p.0,
        field,
        &(target as i64 - field as i64).to_le_bytes(),
    )
}

fn structure(p: &mut Payload, class: u32, bytes: &[u8]) -> usize {
    let start = (p.0.len() + 19) & !15;
    p.0.resize(start - 4, 0);
    p.0.extend(class.to_le_bytes());
    p.0.extend(bytes);
    start
}

/// Uses only the first `value` rows of the array at `field`. The rows after them stay in the
/// record stream, so the array header keeps counting every stored row: the loader walks the
/// stream by header counts, and a header short of its rows makes it read the next row's bytes as
/// a record (fault at `A40343`). Stock descriptors also count fewer rows than their header, such
/// as `80B2EA48 +0x1F8` (56 of 107 `80809A6D` rows).
fn count(p: &mut Payload, field: usize, value: u64) -> Result<()> {
    let header = p.pointer(field + 8)?;
    ensure!(
        value <= p.u64(header)?,
        "An array cannot use more rows than it stores"
    );
    put(&mut p.0, field, &value.to_le_bytes())
}

fn copy_preparation(
    template: &Payload,
    output: &mut Payload,
    from: usize,
    parent: usize,
) -> Result<usize> {
    let si = usize::try_from(template.u64(from + 8)?)?;
    super::pair(template, from, si, [0x808044BE, 0x808044BF])?;
    let i = structure(output, 0x808044BE, &template.0[si..si + 0x190]);
    let d = structure(output, 0x808044BF, &template.0[from..from + 0x320]);
    let owner = output.u32(output.pointer(16)?)?;
    for (instance, definition) in std::iter::once((i, d)).chain(
        [0x28, 0x70, 0xB8, 0x100, 0x160, 0x1A8, 0x1F0]
            .into_iter()
            .map(|field| {
                Ok((
                    i + usize::try_from(template.u64(from + field + 8)?)? - si,
                    d + field,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    ) {
        put(&mut output.0, instance, &owner.to_le_bytes())?;
        put(&mut output.0, definition, &owner.to_le_bytes())?;
        put(
            &mut output.0,
            instance + 8,
            &(definition as u64).to_le_bytes(),
        )?;
        put(
            &mut output.0,
            definition + 8,
            &(instance as u64).to_le_bytes(),
        )?;
    }
    rebase_copied(template, output, owner, [(si, i, 0x190), (from, d, 0x320)])?;
    pointer(output, i + 16, parent)?;
    size(output)?;
    Ok(d)
}

/// Moves every typed reference in the copied spans that still names the template's owner onto
/// the copies. The preparation instance names inline objects of its definition this way, such as
/// the three `80808E76` objects at instance `+0x38`, `+0x48` and `+0x58` of stock `80BC3572`. Left
/// alone, the client resolves them in the template's own payload, where those offsets hold other
/// data. A reference outside both copied spans has no copy to name.
fn rebase_copied(
    template: &Payload,
    output: &mut Payload,
    owner: u32,
    spans: [(usize, usize, usize); 2],
) -> Result<()> {
    let source_owner = template.u32(template.pointer(16)?)?;
    ensure!(
        source_owner != owner,
        "The preparation template and its copy share an owner"
    );
    for &(_, to, length) in &spans {
        for at in (to..to + length - 15).step_by(8) {
            if output.u32(at)? != source_owner || output.u32(at + 4)? & 0xFFFF_0000 != 0x8080_0000 {
                continue;
            }
            let named = usize::try_from(output.u64(at + 8)?)?;
            let moved = spans
                .iter()
                .find(|&&(from, _, length)| (from..from + length).contains(&named))
                .map(|&(from, to, _)| to + (named - from))
                .with_context(|| {
                    format!("Copied reference at {at:X} names {source_owner:08X}+{named:X}")
                })?;
            put(&mut output.0, at, &owner.to_le_bytes())?;
            put(&mut output.0, at + 8, &(moved as u64).to_le_bytes())?;
        }
    }
    Ok(())
}

/// The namespace and metadata of the native preparation's two registered interfaces.
fn providers(entity: &Payload, owner: u32, definition: usize) -> Result<Vec<(u32, usize, u32)>> {
    let maps = entity.array(88, 40, Some(0x80809C22))?;
    let descriptors = entity.array(104, 24, Some(0x80809C20))?;
    ensure!(
        maps.len() == descriptors.len(),
        "Native interface tables disagree"
    );
    let mut found = Vec::new();
    for binding in entity.array(72, 8, Some(0x80809C25))? {
        let key = entity.u32(binding)?;
        if key == u32::MAX {
            continue;
        }
        let span = entity.u32(binding + 4)?;
        let start = (span & 0xFFFF) as usize;
        let end = start + ((span >> 16) & 0x7FFF) as usize;
        ensure!(
            end <= maps.len(),
            "Native interface span is outside its table"
        );
        for index in start..end {
            let map = maps[index];
            let offset = usize::try_from(entity.u64(map + 24)?)?;
            if entity.u32(descriptors[index])? == owner
                && [definition + 0x270, definition + 0x288].contains(&offset)
            {
                ensure!(
                    entity.u32(map)? == 0 && entity.u32(map + 20)? == 0x80809C50,
                    "Native preparation interface has a conditioned or unknown registration"
                );
                found.push((key, offset - definition, entity.u32(map + 32)?));
            }
        }
    }
    ensure!(
        found.len() == 2,
        "Native preparation lacks both interface registrations"
    );
    Ok(found)
}

fn nodes(payloads: &BTreeMap<u32, Payload>, directory: &Path) -> Result<Vec<Value>> {
    let names = payloads
        .keys()
        .map(|tag| (*tag, format!("evade-{tag:08X}")))
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for (&tag, payload) in payloads {
        let mut output = payload.clone();
        let mut patches = Vec::new();
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            let value = payload.u32(at)?;
            let Some(symbol) = names.get(&value) else {
                continue;
            };
            let typed = if at + 16 <= payload.0.len() {
                let class = payload.u32(at + 4)?;
                let offset = usize::try_from(payload.u64(at + 8)?)?;
                let target = &payloads[&value];
                // Package-declared script classes use package handles as their type identities.
                // Their owned object reference must resolve an actual matching class marker.
                offset < target.0.len()
                    && (class & 0xFFFF0000 == 0x80800000
                        || offset
                            .checked_sub(4)
                            .is_some_and(|marker| target.u32(marker).ok() == Some(class)))
            } else {
                false
            };
            let allocation = at == 0x44 && value != tag;
            let component = tag == ENTITY && payload.array(16, 12, Some(0x80809C04))?.contains(&at);
            ensure!(
                typed || allocation || component,
                "Private evade reference {value:08X} at {tag:08X}+{at:X} is not a typed owner reference"
            );
            patches.push(json!({"offset":at,"symbol":symbol}));
            put(&mut output.0, at, &u32::MAX.to_le_bytes())?;
        }
        let file = format!("{}.bin", names[&tag]);
        fs::write(directory.join(&file), &output.0)?;
        result.push(json!({"symbol":names[&tag],"template":tag,"file":file,"patches":patches}));
    }
    Ok(result)
}

/// Prepare a motion experiment from a qualified source profile. This copies no source code
/// into an executable and changes no stock package. The written graph explicitly retains native
/// common energy, body animation and effects. It is not a complete
/// modern subclass import. The caller must keep those limits in the authored item description.
pub fn prepare(
    source: &Payload,
    native: &mut Reader,
    direction: Direction,
    directory: &Path,
) -> Result<Value> {
    let source_profiles = read(source)?;
    let [source_profile] = source_profiles.profiles.as_slice() else {
        anyhow::bail!("The motion experiment requires one unconditional source profile");
    };
    ensure!(
        source_profile.identity == 0x811C9DC5,
        "The source profile needs an explicit private bank selection bridge"
    );
    let source_preparation = source_profile
        .preparation
        .context("Source evade has no preparation")?;
    let entity = native.tag(ENTITY, Some(0x80809C0F))?;
    let graph = Graph::read(&entity, false)?;
    let originals = graph
        .components
        .iter()
        .map(|tag| Ok((*tag, (*native.tag(*tag, Some(0x80809C36))?).clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut owners = originals.clone();
    let movement = native.tag(MOVEMENT, Some(0x80809C36))?;
    let movement_entity = native.tag(MOVEMENT_ENTITY, Some(0x80809C0F))?;
    let md = movement.pointer(24)?;
    let mc = movement.pointer(md + 0x800)?;
    let donor = movement
        .array(mc + 16, 0x80, Some(0x80804106))?
        .into_iter()
        .find(|&row| movement.u64(row + 0x78).is_ok_and(|value| value != 0))
        .context("Native movement has no preparation envelope")?;
    let donor_preparation = movement.pointer(donor + 0x78)?;
    let mut controller = owners
        .remove(&CONTROLLER)
        .context("Native evade controller is absent")?;
    let definition = controller.pointer(24)?;
    let configuration = definition + 0x660;
    let configuration_instance = usize::try_from(controller.u64(configuration + 8)?)?;
    let profiles = controller.array(configuration + 16, 0x80, Some(0x80804106))?;
    let instances = controller.array(configuration_instance + 32, 0x60, Some(0x80804105))?;
    ensure!(
        !profiles.is_empty() && profiles.len() == instances.len(),
        "Native evade profile arrays disagree"
    );
    let profile = profiles[0];
    let runtime = instances[0];
    ensure!(
        controller.u64(profile + 0x78)? == 0
            && controller.u64(runtime + 0x20)? == 0
            && controller.u64(profile + 0x30)? == 0
            && controller.u64(runtime + 0x50)? == 0
            && source_profile.input == controller.u32(profile + 0x70)?,
        "Native evade envelope has another preparation, condition or input contract"
    );
    let mut actions = [0; 4];
    for (index, action) in actions.iter_mut().enumerate() {
        *action = controller.u32(profile + 0x60 + index * 4)?;
    }
    let mut row = controller.bytes::<0x80>(profile)?;
    let effect = controller.u32(profile + 0x50)?;
    super::profile(source_profile, &mut row, effect, actions, direction)?;
    put(&mut controller.0, profile, &row)?;
    count(&mut controller, configuration + 16, 1)?;
    count(&mut controller, configuration_instance + 32, 1)?;
    let preparation = copy_preparation(&movement, &mut controller, donor_preparation, runtime)?;
    pointer(&mut controller, profile + 0x78, preparation)?;
    let preparation_instance = usize::try_from(controller.u64(preparation + 8)?)?;
    pointer(&mut controller, runtime + 0x20, preparation_instance)?;
    let motion = replace_motion(source, &controller, source_preparation, preparation)?;
    controller = motion.owner;
    controller::seal(&mut controller)?;
    owners.insert(CONTROLLER, controller);
    let mut allocation = (*native.tag(originals[&CONTROLLER].u32(0x44)?, None)?).clone();
    Row::replace(
        &[Row::named(
            0x93D202A6,
            u32::MAX,
            0,
            vec![Row::named(
                0xC7532E00,
                0x80804105,
                1,
                vec![Row::named(
                    0x811C9DC5,
                    u32::MAX,
                    0,
                    vec![Row::named(0x1645EC26, 0x808044BE, 1, Vec::new())],
                )],
            )],
        )],
        &mut allocation,
        0x20,
        &[0x93D202A6],
    )?;
    size(&mut allocation)?;
    let provider_rows = providers(&movement_entity, MOVEMENT, donor_preparation)?;
    let metadata = provider_rows
        .iter()
        .map(|(_, _, tag)| Ok((*tag, (*native.tag(*tag, None)?).clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let additional = BTreeMap::from([(
        CONTROLLER,
        provider_rows
            .iter()
            .map(|(key, offset, tag)| compiled::authored::Provider {
                key: *key,
                offset: preparation + offset,
                metadata: &metadata[tag],
                condition: 0,
            })
            .collect(),
    )]);
    let connections = graph.native_rows(
        &graph.components,
        &owners,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let components = graph
        .components
        .iter()
        .map(|tag| compiled::Component {
            owner: *tag,
            payload: &owners[tag],
            template: &originals[tag],
            entity: &entity,
        })
        .collect::<Vec<_>>();
    let compiled = compiled::emit_extended(&entity, &components, &connections, &additional)?;
    let mut payloads = owners;
    for original in originals.values() {
        let tag = original.u32(0x44)?;
        payloads
            .entry(tag)
            .or_insert((*native.tag(tag, None)?).clone());
    }
    payloads.insert(originals[&CONTROLLER].u32(0x44)?, allocation);
    payloads.insert(ENTITY, compiled);
    fs::create_dir(directory).context("Use a fresh motion preparation directory")?;
    let attachments = nodes(&payloads, directory)?;
    let evidence = json!({"source":source_profiles,"motion":motion.evidence,
        "direction":direction,"common_energy":"native_envelope",
        "body_animation":"native_envelope","effects":"native_envelope",
        "source_bank_lowered":false,"gameplay_verified":false});
    let manifest = json!({"attachments":{"installable":true,
        "roots":[format!("evade-{ENTITY:08X}")],"nodes":attachments},
        "evade_translation":evidence});
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(evidence)
}
