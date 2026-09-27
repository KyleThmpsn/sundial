//! Keep independent computed outputs when source model banks reuse a public name.
use super::*;

pub(super) fn merge(
    models: Vec<(String, BTreeMap<String, Channel>)>,
    native: &BTreeMap<String, u8>,
) -> Result<(BTreeMap<String, Channel>, Value)> {
    let mut first = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for (_, channels) in &models {
        for (hash, channel) in channels {
            if first
                .insert(hash.clone(), channel)
                .is_some_and(|old| old != channel)
            {
                conflicts.insert(hash.clone());
            }
        }
    }
    let mut occupied = first
        .keys()
        .chain(native.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    let mut aliases = serde_json::Map::new();
    for (model, channels) in &models {
        let computed = channels
            .iter()
            .flat_map(|(hash, channel)| {
                channel.procedure.iter().flat_map(move |p| {
                    std::iter::once(hash.clone()).chain(p.outputs.iter().cloned())
                })
            })
            .collect::<BTreeSet<_>>();
        let mut names = BTreeMap::new();
        if !computed.is_disjoint(&conflicts) {
            // External live inputs retain their names. Only procedures and their
            // outputs become private, along with every internal reference to them.
            for hash in &computed {
                let private = format!("parhelion_model_{model}_{hash}")
                    .bytes()
                    .fold(0x811C9DC5u32, |n, b| {
                        (n ^ u32::from(b)).wrapping_mul(16_777_619)
                    });
                let private = format!("{private:08X}");
                ensure!(
                    occupied.insert(private.clone()),
                    "private model channel name collision"
                );
                names.insert(hash.clone(), private);
            }
        }
        for (hash, source) in channels {
            let mut channel = source.clone();
            let renamed = |hash: &String| names.get(hash).unwrap_or(hash).clone();
            let target = renamed(hash);
            if target != *hash {
                let old = u32::from_str_radix(hash, 16)?.to_le_bytes();
                let new = u32::from_str_radix(&target, 16)?.to_le_bytes();
                channel.declaration[..4].copy_from_slice(&new);
                for name in [
                    &mut channel.name,
                    &mut channel.resource_name,
                    &mut channel.alias,
                ]
                .into_iter()
                .flatten()
                {
                    ensure!(name[4..] == old, "computed channel property name differs");
                    name[4..].copy_from_slice(&new);
                }
            }
            channel.dependencies = channel.dependencies.iter().map(&renamed).collect();
            channel.resting_procedure = channel.resting_procedure.as_ref().map(renamed);
            if let Some(procedure) = &mut channel.procedure {
                procedure.outputs = procedure.outputs.iter().map(renamed).collect();
            }
            merge_source_channel(&mut result, target, channel)?;
        }
        if !names.is_empty() {
            aliases.insert(model.clone(), json!(names));
        }
    }
    Ok((result, Value::Object(aliases)))
}
