//! Damage events retain their target predicate and actual-damage requirement.
use super::{Node, event, event_modifier};
use crate::d2_mot::{native::effects::resources::selectors, payload::Payload};
use anyhow::{Context, Result, ensure};

pub struct Hit {
    pub node: Node,
    pub selector: selectors::Fragment,
}

/// Source 8080B7B3 and native 80802F5F both qualify the damage event before
/// checking its object selector. Conditional counters and new source state
/// operations require independent translation and are refused here.
pub fn hit_condition(
    source: &Payload,
    at: usize,
    resources: &selectors::Resources,
    groups: &selectors::GroupBindings,
    resolver: &selectors::ResourceResolver,
) -> Result<Hit> {
    ensure!(
        source.u32(at.checked_sub(4).context("hit condition prefix")?)? == 0x8080B7B3,
        "unsupported source hit condition class"
    );
    let root = source.bytes::<360>(at)?;
    let probability = source.f32(at)?;
    ensure!(
        probability.is_finite()
            && (0.0..=1.0).contains(&probability)
            && root[4] == 255
            && root[5] == 4
            && root[6] <= 1
            && root[7] == 0,
        "unsupported source hit condition dispatch or probability"
    );
    ensure!(
        root[0x98] <= 1
            && root[0x99] <= 1
            && root[0x9A..0xA2] == [0; 8]
            && root[0xA2] <= 1
            && root[0xA3..0x128].iter().all(|&byte| byte == 0),
        "source hit condition has unsupported mode, state or ownership filters"
    );
    let threshold = source.f32(at + 0x128)?;
    ensure!(
        threshold.is_finite()
            && root[0x12C] == 0
            && root[0x12D..0x131].iter().all(|&byte| byte <= 1)
            && root[0x131..0x138] == [0; 7]
            && root[0x150..0x154].iter().all(|&byte| byte <= 1)
            && root[0x154..0x168].iter().all(|&byte| byte == 0),
        "source hit condition has unsupported value, counter or extension fields"
    );
    let mut bytes = vec![0; 208];
    bytes[..8].copy_from_slice(&root[..8]);
    let path = event::label_path(&mut bytes);
    event_modifier::damage_filter(source, at + 8, &mut bytes, 8, path)?;
    bytes[0x78..0x80].copy_from_slice(&root[0x80..0x88]);
    bytes[0x80] = 255;
    bytes[0x90..0x92].copy_from_slice(&root[0x98..0x9A]);
    bytes[0x99] = root[0xA2];
    bytes[0x9C..0xA0].copy_from_slice(&0x811C9DC5u32.to_le_bytes());
    bytes[0xA0..0xA4].copy_from_slice(&root[0x128..0x12C]);
    bytes[0xA4..0xA8].copy_from_slice(&root[0x12D..0x131]);
    bytes[0xC0..0xC4].copy_from_slice(&root[0x150..0x154]);
    let mut payload = Payload(bytes);
    selectors::append_labels(
        source,
        (at + 0x28, at + 0x88),
        &mut payload,
        (0x28, 0x80),
        resources,
        groups,
    )?;
    let selector =
        selectors::append_hit_selector(source, at, &mut payload, resources, groups, resolver)?;
    ensure!(
        selector.gates.is_empty(),
        "damage-event selector loses source target categories"
    );
    Ok(Hit {
        node: Node {
            class: 0x80802F5F,
            kind: 4,
            bytes: payload.0,
        },
        selector,
    })
}
