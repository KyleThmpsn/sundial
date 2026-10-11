//! Adapt imported D2 graph publication to the shared native instance layout.
use super::*;
pub(super) use crate::tiger::instance::seal_span;
use crate::tiger::instance::{LAYOUTS, seal};

pub(super) fn finish(graph: &mut Graph) -> Result<()> {
    let mut changed = vec![];
    for layout in LAYOUTS {
        if graph.node(layout.symbol).is_err() {
            continue;
        }
        let mut payload = graph.read(layout.symbol)?;
        let mut patches = graph.node(layout.symbol)?["patches"]
            .as_array()
            .context("component patches")?
            .clone();
        if let Some(evidence) = seal(&mut payload, &mut patches, layout)? {
            graph.write(layout.symbol, &payload.0)?;
            graph.node_mut(layout.symbol)?["patches"] = json!(patches);
            changed.push(evidence);
        }
    }
    if !changed.is_empty() {
        graph.manifest["instance_layout"] = json!(changed);
    }
    if graph.node("object-channels").is_ok() {
        crate::d2_mot::audit::channels::interpolation(
            &graph.read("object-channels")?,
            &graph.read("object-channel-allocation")?,
        )?;
    }
    Ok(())
}
