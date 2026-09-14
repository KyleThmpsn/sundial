//! Shared roles are computed from the nodes that reference an asset, not from the names
//! of every perk that happens to include the same graph.
use super::*;
use crate::sandbox_perk::action::{self, ActionSummary, GroupSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Use {
    operation: String,
    activation: Vec<String>,
    removal: Vec<String>,
}

fn observe(group: &GroupSummary, operation: &str) -> Use {
    Use {
        operation: operation.into(),
        activation: group
            .activation
            .iter()
            .map(|line| line.text.clone())
            .collect(),
        removal: group.removal.iter().map(|line| line.text.clone()).collect(),
    }
}

fn common(uses: &[Use]) -> Option<String> {
    let first = uses.first()?;
    if !uses.iter().all(|usage| usage.operation == first.operation) {
        return Some("Shared Across Different Operations".into());
    }
    let operation = match first.operation.as_str() {
        "Create Entity" => "Attached Entity",
        "Create Entity With Dynamic Value" => "Attached Entity with a Driven Value",
        "Spawn Entity At Selected Transform" => "Spawned Entity",
        "Pattern Override" => "Projectile Pattern",
        operation => operation,
    };
    let same_start = uses
        .iter()
        .all(|usage| usage.activation == first.activation);
    let same_end = uses.iter().all(|usage| usage.removal == first.removal);
    if operation != "Spawned Entity"
        && same_start
        && same_end
        && first.activation == ["The weapon is drawn"]
        && first.removal == ["The weapon is holstered"]
    {
        return Some(format!("{operation} While Drawn"));
    }
    if same_start && first.activation.len() == 1 {
        let trigger = match first.activation[0].as_str() {
            "The weapon is drawn" => "on Draw",
            "The weapon is attached" if operation == "Spawned Entity" => "on Equip",
            "The weapon is attached" => "While Equipped",
            "A kill from this weapon" => "on Weapon Kill",
            "A precision kill from this weapon" => "on Precision Kill",
            "Always" => "Always Active",
            _ => return Some(operation.into()),
        };
        return Some(format!("{operation} {trigger}"));
    }
    Some(operation.into())
}

pub(super) fn annotate(
    manager: &PackageManager,
    index: &dependencies::Index,
    catalog: &mut Catalog,
) {
    let mut uses = BTreeMap::<u32, Vec<Use>>::new();
    let actions = index
        .perks
        .iter()
        .filter_map(|perk| perk.action)
        .collect::<BTreeSet<_>>();
    for tag in actions {
        let Ok(payload) = manager.read_tag(tiger_pkg::TagHash(tag)) else {
            continue;
        };
        let Ok(decoded) = action::decode(&payload) else {
            continue;
        };
        for group in ActionSummary::new(&decoded).groups {
            for effect in &group.effects {
                if let Some(asset) = effect.asset {
                    uses.entry(asset)
                        .or_default()
                        .push(observe(&group, &effect.kind_name));
                }
            }
        }
    }
    for entry in &mut catalog.entries {
        entry.source_hint = uses.get(&entry.graph).and_then(|uses| common(uses));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_hint_keeps_only_roles_common_to_every_observed_use() {
        let drawn = Use {
            operation: "Create Entity".into(),
            activation: vec!["The weapon is drawn".into()],
            removal: vec!["The weapon is holstered".into()],
        };
        assert_eq!(
            common(&[drawn.clone(), drawn.clone()]).as_deref(),
            Some("Attached Entity While Drawn")
        );
        let equipped = Use {
            activation: vec!["The weapon is attached".into()],
            ..drawn.clone()
        };
        assert_eq!(
            common(&[drawn.clone(), equipped]).as_deref(),
            Some("Attached Entity")
        );
        let spawned = Use {
            operation: "Spawn Entity At Selected Transform".into(),
            ..drawn.clone()
        };
        assert_eq!(
            common(&[drawn, spawned]).as_deref(),
            Some("Shared Across Different Operations")
        );
        assert_eq!(common(&[]), None);
    }
}
