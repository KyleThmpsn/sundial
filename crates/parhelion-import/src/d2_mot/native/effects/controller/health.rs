//! Health components retain their region and provider contracts across formats.
use super::*;

mod filter;
mod global;
mod inputs;
mod interfaces;
pub mod policy;
mod pool;
mod programs;
mod provider;
pub mod region;
mod response;
mod root;
mod rules;
mod section;
mod state;

pub struct Health {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    /// The complete graph must reject any endpoint referring to these objects.
    pub unbound: Vec<Object>,
}

pub struct Templates<'a> {
    pub owner: &'a Payload,
    pub allocation: &'a Payload,
    pub scalar_input: &'a Payload,
    pub metadata: &'a std::collections::BTreeMap<u32, Payload>,
}

/// Assemble one health owner. Dependency graphs and category dictionaries are
/// supplied by the enclosing attachment transaction, including recursive graphs.
pub fn emit(
    source: &Payload,
    templates: Templates<'_>,
    bindings: &Bindings,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Health> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid health output identities"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = templates.owner.pointer(16)?;
    let nd = templates.owner.pointer(24)?;
    for (payload, i, d, ic, dc) in [
        (source, si, sd, 0x80804020, 0x80803FFC),
        (templates.owner, ni, nd, 0x80804BEE, 0x80804B8A),
    ] {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64
                && payload.u32(i - 4)? == ic
                && payload.u32(d - 4)? == dc
                && payload.u32(i)? == payload.u32(d)?
                && payload.u32(i + 4)? == dc
                && payload.u32(d + 4)? == ic
                && payload.u64(i + 8)? == d as u64
                && payload.u64(d + 8)? == i as u64,
            "health root pair differs"
        );
    }
    let mut owner = templates.owner.clone();
    let old_tag = owner.u32(ni)?;
    // Retain the native dispatch envelope, including its reciprocal references
    // and any detached template arrays, under the new private identity.
    for at in (0..owner.0.len().saturating_sub(15)).step_by(4) {
        if owner.u32(at)? == old_tag {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "untyped native health owner occurrence"
            );
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
        }
    }
    let mut objects = interfaces::emit(source, &owner, templates.metadata)?;
    root::emit(source, &mut owner, bindings)?;
    let (input_objects, mut unbound) = inputs::emit(source, &mut owner)?;
    objects.extend(input_objects);
    let programs = programs::emit(source, &mut owner, templates.scalar_input)?;
    let sections = section::emit(source, &mut owner, bindings, programs.count)?;
    let regions = region::arrays(source, &mut owner, sections.count)?;
    super::refresh_size(&mut owner)?;
    let providers = provider::emit(source, &mut owner, templates.metadata, regions.count)?;
    global::emit(source, &mut owner, bindings)?;
    let state = state::emit(source, &mut owner, regions.count)?;
    let pool = pool::emit(source, &mut owner)?;
    objects.extend(programs.objects);
    objects.extend(sections.objects);
    objects.extend(regions.objects);
    objects.extend(providers.objects);
    objects.extend(state.objects);
    objects.extend(pool.objects);
    objects.push(object(source, &owner, si, 0x80804020, ni, 0x80804BEE)?);
    objects.push(object(source, &owner, sd, 0x80803FFC, nd, 0x80804B8A)?);
    unbound.extend(sections.unbound);
    unbound.extend(state.unbound);
    let mut rows = vec![
        allocation::Row::named(0xCCD237B9, 0x80804BAB, sections.count, Vec::new()),
        allocation::Row::named(0x1D69DBFF, 0x80804C5E, regions.count, Vec::new()),
    ];
    if programs.count != 0 {
        rows.push(allocation::Row::named(
            0x6CE653CE,
            0x808089F7,
            programs.count,
            programs.allocation,
        ));
    }
    if providers.count != 0 {
        rows.push(allocation::Row::named(
            0xA005FA37,
            0x80804B91,
            providers.count,
            Vec::new(),
        ));
    }
    if pool.count != 0 {
        rows.push(allocation::Row::named(
            0xEA198763,
            0x80804C42,
            1,
            vec![allocation::Row::named(
                0x811C9DC5,
                u32::MAX,
                0,
                vec![allocation::Row::named(
                    0x298772D9,
                    0x80804C45,
                    pool.count,
                    Vec::new(),
                )],
            )],
        ));
    }
    rows.extend(state.allocation);
    let mut allocation = templates.allocation.clone();
    allocation::Row::write(&rows, &mut allocation, 0x20)?;
    put(&mut owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    super::seal(&mut owner)?;
    Ok(Health {
        owner,
        allocation,
        objects,
        unbound,
    })
}

pub struct Bindings {
    /// Source payload field to a class-checked native dependency.
    pub resources: std::collections::BTreeMap<usize, crate::d2_mot::entity::sequence::Resource>,
    pub categories: crate::d2_mot::native::effects::resources::selectors::Resources,
}

fn resource(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    classes: (u32, u32),
    bindings: &Bindings,
) -> Result<()> {
    let empty = source.u32(from)? == u32::MAX
        && matches!(source.u32(from + 4)?, 1 | 2)
        && source.u64(from + 8)? == 0;
    let target = if empty {
        ensure!(
            !bindings.resources.contains_key(&from),
            "empty health dependency has a binding"
        );
        u32::MAX
    } else {
        let binding = bindings
            .resources
            .get(&from)
            .with_context(|| format!("health dependency at {from:X} is unbound"))?;
        ensure!(
            binding.source_class == classes.0
                && binding.native_class == classes.1
                && (0x80800001..=0x81FFFFFF).contains(&binding.tag),
            "health dependency class or target differs"
        );
        binding.tag
    };
    put(&mut output.0, to, &target.to_le_bytes())?;
    put(&mut output.0, to + 4, &0u32.to_le_bytes())
}

fn typed(output: &mut Payload, class: u32, size: usize) -> Result<usize> {
    let at = output
        .0
        .len()
        .checked_add(11)
        .context("health record alignment")?
        & !7;
    output
        .0
        .resize(at.checked_add(size).context("health record extent")?, 0);
    put(&mut output.0, at - 4, &class.to_le_bytes())?;
    Ok(at)
}

fn string(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    put(&mut output.0, to, &0u64.to_le_bytes())?;
    if source.u64(from)? == 0 {
        return Ok(());
    }
    let start = source.pointer(from)?;
    let bytes = source.0.get(start..).context("health string extent")?;
    let length = bytes
        .iter()
        .position(|&byte| byte == 0)
        .context("health string terminator")?;
    ensure!(length <= 4096, "health string exceeds limit");
    std::str::from_utf8(&bytes[..length]).context("health string encoding")?;
    let at = typed(output, 0x80800065, 8 + length + 1)?;
    put(&mut output.0, at, &((length + 1) as u64).to_le_bytes())?;
    put(&mut output.0, at + 8, &bytes[..length + 1])?;
    relative(output, to, at + 8)
}

fn relative(output: &mut Payload, field: usize, target: usize) -> Result<()> {
    let delta = i64::try_from(target)? - i64::try_from(field)?;
    put(&mut output.0, field, &delta.to_le_bytes())
}

fn pair(
    output: &mut Payload,
    instance: usize,
    definition: usize,
    instance_class: u32,
    definition_class: u32,
    parent: usize,
) -> Result<()> {
    let owner = output.u32(parent)?;
    for (at, class, twin) in [
        (instance, definition_class, definition),
        (definition, instance_class, instance),
    ] {
        put(&mut output.0, at, &owner.to_le_bytes())?;
        put(&mut output.0, at + 4, &class.to_le_bytes())?;
        put(&mut output.0, at + 8, &(twin as u64).to_le_bytes())?;
    }
    relative(output, instance + 16, parent)
}

fn check_pair(
    p: &Payload,
    instance: usize,
    definition: usize,
    ic: u32,
    dc: u32,
    parent: usize,
) -> Result<()> {
    let owner = p.u32(p.pointer(16)?)?;
    ensure!(
        p.u32(instance)? == owner
            && p.u32(definition)? == owner
            && p.u32(instance + 4)? == dc
            && p.u32(definition + 4)? == ic
            && p.u64(instance + 8)? == definition as u64
            && p.u64(definition + 8)? == instance as u64
            && p.pointer(instance + 16)? == parent,
        "health record pair or parent differs at {instance:X}"
    );
    Ok(())
}

fn object(
    source: &Payload,
    output: &Payload,
    from: usize,
    sc: u32,
    to: usize,
    nc: u32,
) -> Result<Relocation> {
    Ok(Relocation {
        source: Object {
            owner: source.u32(source.pointer(16)?)?,
            class: sc,
            offset: from as u64,
        },
        target: Object {
            owner: output.u32(output.pointer(16)?)?,
            class: nc,
            offset: to as u64,
        },
    })
}
