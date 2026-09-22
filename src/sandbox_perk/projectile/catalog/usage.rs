//! Shared roles are computed from the nodes that reference an asset, not from the names
//! of every perk that happens to include the same graph.
use super::*;
use crate::sandbox_perk::action::{self, ActionSummary, GroupSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Use {
    kind: u8,
    activation: Vec<String>,
    removal: Vec<String>,
}

fn observe(group: &GroupSummary, kind: u8) -> Use {
    Use {
        kind,
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
    if !uses.iter().all(|usage| usage.kind == first.kind) {
        return Some("Shared Across Different Operations".into());
    }
    let operation = match first.kind {
        1 => "Attached Entity",
        2 => "Attached Entity with a Driven Value",
        3 => "Spawned Entity",
        26 => "Projectile Pattern",
        kind => crate::sandbox_perk::nodes::effect(kind)?.name,
    };
    let same_start = uses
        .iter()
        .all(|usage| usage.activation == first.activation);
    let same_end = uses.iter().all(|usage| usage.removal == first.removal);
    // Kind 1's cleanup key and entity components can let the attachment outlive
    // this action. Its activation time is known, its lifetime is not implied.
    if matches!(first.kind, 2 | 26)
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
            "The weapon is attached" if matches!(first.kind, 1 | 3) => "on Equip",
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
                if let Some(asset) = effect.asset
                    && let Some((false, native)) = &effect.native
                {
                    uses.entry(asset)
                        .or_default()
                        .push(observe(&group, native.kind));
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
            kind: 1,
            activation: vec!["The weapon is drawn".into()],
            removal: vec!["The weapon is holstered".into()],
        };
        assert_eq!(
            common(&[drawn.clone(), drawn.clone()]).as_deref(),
            Some("Attached Entity on Draw")
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
            kind: 3,
            ..drawn.clone()
        };
        assert_eq!(
            common(&[drawn, spawned]).as_deref(),
            Some("Shared Across Different Operations")
        );
        assert_eq!(common(&[]), None);
    }

    #[test]
    fn spawned_and_driven_roles_follow_native_kinds() {
        let usage = Use {
            kind: 3,
            activation: vec!["The weapon is attached".into()],
            removal: Vec::new(),
        };
        assert_eq!(
            common(&[usage.clone()]).as_deref(),
            Some("Spawned Entity on Equip")
        );
        assert_eq!(
            common(&[Use { kind: 2, ..usage }]).as_deref(),
            Some("Attached Entity with a Driven Value While Equipped")
        );
    }
}
