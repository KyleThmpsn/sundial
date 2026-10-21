use super::*;

pub(in crate::app) struct Contribution {
    name: String,
    value: i32,
}

pub(in crate::app) type Contributions = BTreeMap<u16, Vec<Contribution>>;

/// Declared stat amounts on each socket's starting plug. A private replacement supplies its
/// complete stat list. Other choices in the socket do not contribute simultaneously.
pub(in crate::app) fn starting_plugs(
    catalog: Option<&InvestmentCatalog>,
    recipe: &WeaponRecipe,
    donor: &WeaponDonor,
) -> Contributions {
    let mut contributions = Contributions::new();
    let sockets = super::super::socket_editor::socket_editor_donor(donor, recipe);
    for socket in &sockets.sockets {
        let column = recipe
            .overrides
            .socket_columns
            .get(socket.index)
            .and_then(Option::as_ref);
        if column
            .and_then(|column| column.socket_type)
            .unwrap_or(socket.socket_type)
            == u16::MAX
        {
            continue;
        }
        let plug = column.map_or(socket.native_default, |column| {
            column
                .choices
                .first()
                .and_then(|hash| hash.parse_u32().ok())
        });
        let Some(plug) = plug else { continue };
        let variant = recipe
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| {
                usize::from(variant.socket_index) == socket.index
                    && variant.choice_index == 0
                    && variant.source_plug_hash.parse_u32().ok() == Some(plug)
            });
        let name = variant
            .and_then(|variant| variant.name.clone())
            .unwrap_or_else(|| {
                catalog.map_or_else(
                    || format!("Plug 0x{plug:08X}"),
                    |catalog| catalog.plug_label(plug, false),
                )
            });
        let stats = match variant {
            Some(variant) if variant.replace_effects => variant
                .investment_stats
                .iter()
                .map(|stat| (stat.definition_index, stat.value))
                .collect::<Vec<_>>(),
            _ => catalog
                .map(|catalog| catalog.item_stat_contributions(plug))
                .unwrap_or_default()
                .into_iter()
                .map(|stat| (stat.definition_index, stat.value))
                .collect(),
        };
        for (index, value) in stats {
            if value != 0 {
                contributions.entry(index).or_default().push(Contribution {
                    name: name.clone(),
                    value,
                });
            }
        }
    }
    contributions
}

pub(super) fn total(own: i32, sources: &[Contribution]) -> i32 {
    let total = sources
        .iter()
        .fold(i64::from(own), |sum, source| sum + i64::from(source.value));
    total.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(super) fn breakdown(own: i32, sources: &[Contribution], shown: i32) -> String {
    let mut lines = vec![format!("Base {own}")];
    lines.extend(
        sources
            .iter()
            .map(|source| format!("{} {:+}", source.name, source.value)),
    );
    if shown != total(own, sources) {
        lines.push(format!("Shows {shown}"));
    }
    lines.join("\n")
}
