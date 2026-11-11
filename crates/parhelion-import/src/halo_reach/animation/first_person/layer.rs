//! Keep the donor's action-coupled layers out of complete translated poses.
use super::*;
use crate::tiger::animation::layer::{ACTION_COUPLED, omit_action_coupled};

/// Translated actions take descriptors from `first` on. Their routes omit the layers that
/// couple the donor's own actions, as [`ACTION_COUPLED`] describes. Halo: Reach motion is never a
/// revision of a donor clip, so no layer fits it.
pub(super) fn source_actions(
    native: &mut Reader,
    table: &Payload,
    bank: &Payload,
    first: usize,
) -> Result<(Payload, Value)> {
    let aliases = table.array(40, 2, Some(0x80800006))?.len();
    ensure!(
        first <= aliases,
        "Translated action dispatch is outside its table"
    );
    let descriptors = (first..aliases)
        .map(|descriptor| (descriptor, None))
        .collect::<BTreeMap<_, _>>();
    let (table, omitted, _, added) = omit_action_coupled(native, table, bank, &descriptors)?;
    Ok((
        table,
        json!({"layers":ACTION_COUPLED.map(|name| format!("{name:08X}")),
        "omitted":omitted.iter().map(|name| format!("{name:08X}")).collect::<Vec<_>>(),"removed_routes":added,
        "method":"Complete source actions omit the donor's action-coupled layers. Procedural layers and original action dispatch remain intact."}),
    ))
}
