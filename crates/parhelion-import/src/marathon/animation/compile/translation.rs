//! Translation of every clip the routes reach, and which actions keep a source route. An action
//! whose route has any choice that did not translate falls back to the carrier's state. A
//! required action that would fall back stops the compile.
use super::pairing::WeaponLocal;
use super::plan::{Routes, Sources};
use super::*;

/// Source clip, weapon-local descriptor, action, source descriptor and correction variant.
/// Equal base clips can need different constant corrections under different runtime inputs.
pub(super) type Key = (usize, Option<u32>, u32, u32, u32);

pub(super) struct Translation {
    /// The clip each action's source descriptor plays.
    pub(super) keys: BTreeMap<(u32, u32), Key>,
    /// Each clip's authoring and its report, or the reason it failed.
    pub(super) authored: BTreeMap<Key, std::result::Result<(Authored, Value), String>>,
    /// Actions that keep the carrier's state, with the reason.
    pub(super) unsupported: Vec<(u32, String)>,
}

impl Translation {
    /// Translates the clips `routes` reach, keeping each failure with its clip, then removes
    /// from `routes` every action whose route has a choice that did not translate.
    pub(super) fn convert(
        source: &mut Reader,
        sources: &Sources,
        routes: &mut Routes,
        local: &WeaponLocal,
        retarget: &retarget::Retarget,
    ) -> Result<Self> {
        let controller = &sources.controller;
        let mut keys = BTreeMap::<(u32, u32), Key>::new();
        let mut authored = BTreeMap::new();
        let mut variants = BTreeMap::new();
        for (&request, route) in &routes.routes {
            let action = routes.requests[&request].action;
            for &(descriptor, _) in route {
                let entry = &controller.descriptors[descriptor as usize];
                let Some(ordinal) = entry.clip else {
                    continue;
                };
                let tag = controller.clips[ordinal];
                let original = source.tag(tag, Some(0x8080AE01))?;
                let overlay = local.overlays.get(&(request, descriptor)).copied();
                let selected = sources
                    .fixed_layers
                    .selection(descriptor, &routes.requests[&request].inputs)?;
                let next = u32::try_from(variants.len())?;
                let variant = *variants.entry(selected.to_vec()).or_insert(next);
                let key = (ordinal, overlay, action, descriptor, variant);
                keys.insert((request, descriptor), key);
                if authored.contains_key(&key) {
                    continue;
                }
                let overlay = overlay.map(|descriptor| local.overlay(descriptor));
                let aimed = AIMED.iter().any(|name| source::fnv(name) == action);
                let attempt = clip::lower(&original, Some(&sources.carrier.vocabulary)).and_then(
                    |(candidate, lowered)| {
                        let result = author(
                            &candidate,
                            sources,
                            retarget,
                            &routes.targets,
                            overlay.as_ref(),
                            &sources.fixed_layers.get(selected),
                            aimed,
                        )?;
                        Ok((result, json!({"source":tag,"name":format!("{:08X}",original.u32(0x120)?),"lowering":lowered})))
                    },
                );
                authored.insert(
                    key,
                    attempt.map_err(|error| format!("Source clip {tag:08X}: {error:#}")),
                );
            }
        }
        let required = REQUIRED
            .iter()
            .map(|a| source::fnv(a))
            .collect::<BTreeSet<_>>();
        let mut unsupported = BTreeMap::new();
        for (&request, route) in &routes.routes {
            let reason = route.iter().find_map(|&(descriptor, _)| {
                let entry = &controller.descriptors[descriptor as usize];
                match entry.clip {
                    None => Some("Selects an unbound source descriptor".to_owned()),
                    Some(_) if entry.rate != 1. => {
                        Some("Source descriptor playback rate is untranslated".to_owned())
                    }
                    Some(_) => authored[&keys[&(request, descriptor)]]
                        .as_ref()
                        .err()
                        .cloned(),
                }
            });
            if let Some(reason) = reason {
                unsupported
                    .entry(routes.requests[&request].action)
                    .or_insert(reason);
            }
        }
        // One unusable branch keeps the entire action native. No native input can reach an
        // untranslated descriptor, including an input combination absent from the default route.
        routes
            .actions
            .retain(|action, _| !unsupported.contains_key(action));
        routes
            .routes
            .retain(|request, _| !unsupported.contains_key(&routes.requests[request].action));
        for (action, reason) in &unsupported {
            ensure!(
                !required.contains(action),
                "Required action {action:08X} has no complete translation: {reason}"
            );
        }
        Ok(Self {
            keys,
            authored,
            unsupported: unsupported.into_iter().collect(),
        })
    }
}
