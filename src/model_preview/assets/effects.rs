//! Authored order and stored timing of an effect sequence's native nodes. Branching and
//! runtime scheduling rules still depend on engine behavior outside the component.
use super::*;

const EFFECT_HEADER: u32 = 0x8080_84D7;
const EFFECT_DATA: u32 = 0x8080_84E9;
const NODE_ARRAY: u32 = 0x8080_93E6;
const NODE_ROW: u32 = 0x8080_93E5;
const PARTICLE_NODE: u32 = 0x8080_6CC5;
const SOUND_NODE: u32 = 0x8080_6B37;

pub(crate) struct Node {
    pub source: u32,
    pub index: usize,
    pub class: u32,
    pub target: Option<u32>,
    pub timing: Option<Timing>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Timing {
    pub start: f32,
    pub duration: f32,
}

impl Node {
    pub fn kind(&self) -> &'static str {
        match self.class {
            PARTICLE_NODE => "Particle",
            SOUND_NODE => "Sound",
            _ => "Effect Node",
        }
    }
}

pub(super) fn read(manager: &PackageManager, components: &[Component]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for component in components {
        if component.header != Some(EFFECT_HEADER) || component.data != Some(EFFECT_DATA) {
            continue;
        }
        let Some(entry) = manager.get_entry(component.tag) else {
            continue;
        };
        if entry.reference != RESOURCE || entry.file_type != 8 || entry.file_size > 2 * 1024 * 1024
        {
            continue;
        }
        let Ok(bytes) = manager.read_tag(component.tag) else {
            continue;
        };
        if u64_at(&bytes, 0).ok() != Some(bytes.len() as u64) {
            continue;
        }
        if let Ok(found) = read_component(manager, component.tag, &bytes) {
            nodes.extend(found);
        }
    }
    nodes
}

fn read_component(
    manager: &PackageManager,
    source: u32,
    bytes: &[u8],
) -> Result<Vec<Node>, String> {
    let header = pointer(bytes, 0x10)?;
    let data = pointer(bytes, 0x18)?;
    if header < 4
        || data < 4
        || u32_at(bytes, header - 4)? != EFFECT_HEADER
        || u32_at(bytes, data - 4)? != EFFECT_DATA
    {
        return Err("Effect component has an unexpected layout".into());
    }
    let descriptor = data
        .checked_add(0x168)
        .ok_or("Effect node offset overflow")?;
    let (count, rows, class) = array_at(bytes, descriptor)?;
    if class != NODE_ARRAY || count > 512 {
        return Err("Effect node array has an unexpected layout".into());
    }
    let rows_end = rows
        .checked_add(count.checked_mul(24).ok_or("Effect node count overflow")?)
        .ok_or("Effect node rows overflow")?;
    if rows_end > bytes.len() {
        return Err("Effect node array exceeds its component".into());
    }
    let mut nodes = Vec::with_capacity(count);
    for index in 0..count {
        let row = rows + index * 24;
        if u32_at(bytes, row)? != source || u32_at(bytes, row + 4)? != NODE_ROW {
            return Err("Effect node row has an unexpected owner or class".into());
        }
        let node = pointer(bytes, row + 16)?;
        let class = u32_at(bytes, node + 4)?;
        let target = match class {
            SOUND_NODE => target(manager, bytes, node + 0x40, &[SOUND, SOUND_COLLECTION]),
            PARTICLE_NODE => target(manager, bytes, node + 0x158, &[PARTICLE_SYSTEM])
                .or_else(|| target(manager, bytes, node + 0x150, &[PARTICLE_SYSTEM])),
            _ => None,
        };
        let timing = matches!(class, SOUND_NODE | PARTICLE_NODE)
            .then(|| read_timing(bytes, node))
            .flatten();
        nodes.push(Node {
            source,
            index,
            class,
            target,
            timing,
        });
    }
    Ok(nodes)
}

fn read_timing(bytes: &[u8], node: usize) -> Option<Timing> {
    let value = |offset| u32_at(bytes, node + offset).ok().map(f32::from_bits);
    let timing = Timing {
        start: value(0x20)?,
        duration: value(0x28)?,
    };
    (timing.start.is_finite()
        && timing.duration.is_finite()
        && timing.start >= 0.0
        && timing.duration >= 0.0)
        .then_some(timing)
}

fn target(manager: &PackageManager, bytes: &[u8], at: usize, classes: &[u32]) -> Option<u32> {
    let tag = u32_at(bytes, at).ok()?;
    manager
        .get_entry(tag)
        .is_some_and(|entry| entry.file_type == 8 && classes.contains(&entry.reference))
        .then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and installed Shadowkeep packages"]
    fn installed_effect_nodes_preserve_air_weak_authoring_order() {
        let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
        let manager = crate::investment::discovery::open_packages(std::path::Path::new(&packages))
            .expect("installed packages");
        let bytes = manager.read_tag(0x80EF_576A).expect("effect component");
        let nodes = read_component(&manager, 0x80EF_576A, &bytes).expect("effect nodes");
        assert_eq!(nodes.len(), 7);
        assert_eq!(nodes[0].class, PARTICLE_NODE);
        assert_eq!(nodes[0].target, Some(0x80EF_5769));
        assert_eq!(nodes[0].timing.unwrap().start, 0.0);
        assert_eq!(nodes[0].timing.unwrap().duration, 0.0001);
        assert_eq!(nodes[3].class, SOUND_NODE);
        assert_eq!(nodes[3].target, Some(0x80BB_E621));
        assert_eq!(nodes[3].timing.unwrap().duration, 0.0);
        assert_eq!(nodes[6].target, Some(0x80BB_E626));

        let delayed = manager
            .read_tag(0x8153_3070)
            .expect("delayed effect component");
        let delayed = read_component(&manager, 0x8153_3070, &delayed).unwrap();
        assert_eq!(delayed[1].timing.unwrap().start, 0.15);
        assert_eq!(delayed[2].timing.unwrap().start, 0.25);

        let ranged = manager
            .read_tag(0x8153_3D86)
            .expect("ranged effect component");
        let ranged = read_component(&manager, 0x8153_3D86, &ranged).unwrap();
        assert_eq!(ranged[12].timing.unwrap().duration, 0.6);
    }
}
