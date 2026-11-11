//! Translation of every clip the routes reach, and which actions keep a source route. An action
//! whose route has any choice that did not translate falls back to the carrier's state. A
//! required action that would fall back stops the compile.
use super::pairing::WeaponLocal;
use super::plan::{Routes, Sources};
use super::*;

/// A source clip ordinal with the weapon-local descriptor that accompanies it. An absolute clip
/// is authored once for each weapon-local descriptor.
pub(super) type Key = (usize, Option<u32>);

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
        for (&action, route) in &routes.routes {
            for &(descriptor, _) in route {
                let entry = &controller.descriptors[descriptor as usize];
                let Some(ordinal) = entry.clip else {
                    continue;
                };
                let tag = controller.clips[ordinal];
                let original = source.tag(tag, Some(0x8080AE01))?;
                let overlay = local.overlays.get(&(action, descriptor)).copied();
                let key = (ordinal, overlay);
                keys.insert((action, descriptor), key);
                if authored.contains_key(&key) {
                    continue;
                }
                let overlay = overlay.map(|descriptor| local.overlay(descriptor));
                let attempt = clip::lower(&original, Some(&sources.carrier.vocabulary)).and_then(
                    |(candidate, lowered)| {
                        let result = author(
                            &candidate,
                            &sources.arms,
                            retarget,
                            &sources.native_rig,
                            &sources.extension.controls,
                            &routes.targets,
                            overlay.as_ref(),
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
        let mut unsupported = Vec::new();
        routes.routes.retain(|action, route| {
            let reason = route.iter().find_map(|&(descriptor, _)| {
                let entry = &controller.descriptors[descriptor as usize];
                match entry.clip {
                    None => Some("Selects an unbound source descriptor".to_owned()),
                    Some(_) if entry.rate != 1. => {
                        Some("Source descriptor playback rate is untranslated".to_owned())
                    }
                    Some(_) => authored[&keys[&(*action, descriptor)]]
                        .as_ref()
                        .err()
                        .cloned(),
                }
            });
            if let Some(reason) = reason {
                unsupported.push((*action, reason));
                false
            } else {
                true
            }
        });
        for (action, reason) in &unsupported {
            ensure!(
                !required.contains(action),
                "Required action {action:08X} has no complete translation: {reason}"
            );
        }
        Ok(Self {
            keys,
            authored,
            unsupported,
        })
    }
}
