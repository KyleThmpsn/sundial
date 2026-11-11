//! Checked native audio routes for ordinary browser imports and explicit plans.
use super::*;

// The primary player-fire bus observed in Shadowkeep weapon presentation banks.
// Use it only when the selected bank declares it in a checked HIRC node.
const PLAYER_FIRE: u32 = 2_196_017_938;

/// Automatic imports must not silently discard a distinct source activation path.
pub(super) fn validate_firing_routes(behavior: &Value) -> Result<()> {
    let (primary, _) = fire_sounds(behavior)?;
    let selected = primary
        .iter()
        .filter_map(|sound| sound["tag"]["datum"].as_u64())
        .collect::<BTreeSet<_>>();
    let nodes = behavior["nodes"]
        .as_object()
        .context("Source behavior nodes")?;
    let edges = behavior["edges"]
        .as_array()
        .context("Source behavior edges")?;
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Source behavior root")?;
    // A trigger's secondary barrel is its second firing action, such as a charged release.
    // The native weapon fires once per activation and has no second action to carry it.
    let secondary = nodes
        .get(&format!("{root:08X}"))
        .and_then(|node| node["data"]["object"]["gameplay"]["triggers"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|trigger| trigger["secondary_barrel"].as_i64())
        .filter(|barrel| *barrel >= 0)
        .collect::<BTreeSet<_>>();
    // A charged pair's charged barrel is the one second action the importer translates, as the
    // charged release perk, whose receipt records it. That shot plays the weapon's own firing
    // route, and the primary fire receipt lists the barrel's recording as untranslated. A
    // Spew-Charge pair fires its charged barrel as the weapon's shot, and its spew is not fired.
    let charged = nodes
        .get(&format!("{root:08X}"))
        .and_then(|node| super::super::firing::charged_pair(&node["data"]["object"]["gameplay"]))
        .and_then(|pair| i64::try_from(pair.other()).ok());
    for edge in edges.iter().filter(|edge| {
        edge["owner"] == behavior["root"]["datum"]
            && matches!(
                edge["role"].as_str(),
                Some("fire" | "secondary_fire" | "charge" | "discharge")
            )
            && !(edge["role"] == "fire" && charged.is_some() && edge["barrel"].as_i64() == charged)
    }) {
        let mut queue = vec![
            edge["tag"]["datum"]
                .as_u64()
                .context("Firing route identity")?,
        ];
        let mut seen = BTreeSet::new();
        while let Some(id) = queue.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(node) = nodes.get(&format!("{id:08X}")) else {
                continue;
            };
            match node["tag"]["group"].as_str() {
                Some("snd!") => {
                    // The primary route is intentionally first-person. Distant and
                    // third-person layers are retained as separate source evidence.
                    if node["data"]["class"] == 4 || edge["role"] != "fire" {
                        let action = edge["barrel"]
                            .as_i64()
                            .filter(|barrel| secondary.contains(barrel))
                            .map_or(String::new(), |barrel| {
                                format!(" on the trigger's secondary barrel {barrel}")
                            });
                        ensure!(
                            edge["role"] == "fire" && selected.contains(&id),
                            "Source firing audio route {}{action} for {} has no translated native activation. An explicit route is required",
                            edge["role"],
                            node["tag"]["path"]
                        );
                    }
                }
                Some("effe" | "lsnd") => {
                    queue.extend(
                        edges
                            .iter()
                            .filter(|link| link["owner"] == id)
                            .filter_map(|link| link["tag"]["datum"].as_u64()),
                    );
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn inherited_bus(objects: &BTreeMap<u32, (u8, &[u8])>, mut target: u32) -> Result<u32> {
    let mut seen = BTreeSet::new();
    loop {
        ensure!(seen.insert(target), "Native audio parent chain cycles");
        let (kind, node) = objects
            .get(&target)
            .context("Native audio parent is outside its bank")?;
        ensure!(
            matches!(kind, 2 | 5 | 6 | 7 | 9),
            "Native firing target is not a sound node"
        );
        let base = if *kind == 2 { 18 } else { 4 };
        ensure!(
            node.get(base..base + 3) == Some(&[0, 0, 0]),
            "Native firing bus inheritance requires effect-chain translation. Configure fire_bus"
        );
        let bus = word(node, base + 3)?;
        if !matches!(bus, 0 | u32::MAX) {
            return Ok(bus);
        }
        target = word(node, base + 7)?;
        ensure!(
            !matches!(target, 0 | u32::MAX),
            "Native firing bus inheritance has no output"
        );
    }
}

pub(super) fn output_bus(bank: &[u8], explicit: Option<u32>, event: u32) -> Result<u32> {
    if let Some(bus) = explicit {
        verify_bus(bank, bus)?;
        return Ok(bus);
    }
    let objects = bank_objects(bank)?;
    let (kind, node) = objects
        .get(&event)
        .context("Native firing event is absent from its bank")?;
    ensure!(*kind == 4, "Native firing cue does not select an event");
    let count = word(node, 4)? as usize;
    ensure!(
        node.len() == 8 + count * 4,
        "Native firing event extent differs"
    );
    let mut buses = BTreeSet::new();
    for index in 0..count {
        let action = word(node, 8 + index * 4)?;
        let (kind, action) = objects
            .get(&action)
            .context("Native firing action is absent")?;
        ensure!(*kind == 3, "Native firing event selects a non-action");
        let action_type = u16::from_le_bytes(
            action
                .get(4..6)
                .context("Native firing action type")?
                .try_into()?,
        );
        if action_type & 0xff00 == 0x0400 {
            buses.insert(inherited_bus(&objects, word(action, 6)?)?);
        }
    }
    ensure!(
        buses.len() == 1,
        "Native firing output bus is absent or ambiguous. Configure fire_bus"
    );
    Ok(*buses.first().unwrap())
}

pub(super) fn resolve(
    reader: &mut crate::tiger::reader::Reader,
    rig: &Value,
    behavior: &Value,
    supplied: &Options,
) -> Result<Options> {
    let mut options = supplied.clone();
    // An explicit bus selects the ordinary checked firing route. Automatic named
    // and loop discovery must not replace that authored selection.
    if options.firing.is_some() || options.fire_bus.is_some() {
        return Ok(options);
    }
    let groups = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Native audio presentation")?;
    let sounds = groups
        .iter()
        .flat_map(|group| group["sounds"].as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    let attached = firing_loops(behavior)?;
    if !attached.is_empty() {
        // Each presentation group lists its own sounds, so a loop shared by the item's content rows
        // appears once per group. The item's selected content row decides, and a loop node counts
        // once however many groups list it.
        let loops = move |selected: bool| {
            groups
                .iter()
                .filter(|group| !selected || group["content_key"] == rig["content_key"])
                .flat_map(|group| group["sounds"].as_array().into_iter().flatten())
                .filter(|sound| {
                    sound["root_slot"] == 4
                        && sound["component_class"] == "808084E9"
                        && sound["node_kind"] == "loop"
                })
                .filter_map(|sound| sound["node_name"].as_str())
                .collect::<BTreeSet<_>>()
        };
        let selected = loops(true);
        let loops = if selected.is_empty() {
            loops(false)
        } else {
            selected
        };
        if let ([source], [node]) = (
            attached.as_slice(),
            loops.iter().collect::<Vec<_>>().as_slice(),
        ) {
            options.firing = Some(Firing::Loop {
                source: source["tag"]["path"]
                    .as_str()
                    .context("Source loop path")?
                    .into(),
                slot: 4,
                node: u32::from_str_radix(node, 16)?,
            });
            return Ok(options);
        }
        // A donor with no native loop cannot carry the source loop, such as a charging hum on a
        // weapon whose donor fires once per shot. The source's one-shot fire still takes the
        // ordinary route below, and the receipt names the loop it leaves out. Several loops on
        // either side stay ambiguous.
        ensure!(
            loops.is_empty() && !fire_sounds(behavior)?.0.is_empty(),
            "Source or native firing loop is ambiguous: source loops {:?}, native slot 4 loops {:?}. Configure the explicit loop route",
            attached
                .iter()
                .map(|node| node["tag"]["path"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            loops
        );
    }
    // Native named cues carry event paths. A terminal fire token excludes ready,
    // reload, tail and handling events. Ambiguous paths still need an explicit route.
    let named = rig["audio"]["named_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["events"].as_array().into_iter().flatten())
        .filter(|event| {
            event["sounds"].as_array().is_some_and(|sounds| {
                sounds.len() == 1
                    && sounds[0]["event_path"].as_str().is_some_and(|path| {
                        path.to_ascii_lowercase()
                            .strip_suffix(".wwise_event")
                            .is_some_and(|stem| stem.ends_with("_fire"))
                    })
            })
        })
        .collect::<Vec<_>>();
    if named.len() == 1 {
        options.firing = Some(Firing::Named {
            event: u32::from_str_radix(
                named[0]["hash"].as_str().context("Native firing event")?,
                16,
            )?,
        });
    } else if options.fire_bus.is_none() {
        let mut primary = Vec::new();
        for sound in sounds.iter().filter(|sound| {
            sound["root_slot"] == 0
                && sound["component_class"] == "808084E9"
                && sound["node_kind"] == "once"
        }) {
            let bank =
                u32::from_str_radix(sound["bank"].as_str().context("Native firing bank")?, 16)?;
            if bank_has_bus(&reader.tag(bank, None)?.0, PLAYER_FIRE)? {
                primary.push(*sound);
            }
        }
        if primary.len() == 1 {
            options.fire_bus = Some(PLAYER_FIRE);
        }
    }
    Ok(options)
}

pub(super) fn has_firing_loop(behavior: &Value) -> Result<bool> {
    Ok(!firing_loops(behavior)?.is_empty())
}

fn firing_loops(behavior: &Value) -> Result<Vec<&Value>> {
    let nodes = behavior["nodes"]
        .as_object()
        .context("Source behavior nodes")?;
    let loops = behavior["edges"]
        .as_array()
        .context("Source attachment edges")?
        .iter()
        .filter(|edge| edge["owner"] == behavior["root"]["datum"] && edge["role"] == "attachment")
        .filter_map(|edge| edge["tag"]["datum"].as_u64())
        .filter_map(|id| nodes.get(&format!("{id:08X}")))
        .filter(|node| node["tag"]["group"] == "lsnd")
        .filter(|node| {
            node["data"]["track_controls"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|track| {
                    track["loop"]["datum"]
                        .as_u64()
                        .and_then(|id| nodes.get(&format!("{id:08X}")))
                        .is_some_and(|sound| sound["data"]["class"] == 4)
                })
        })
        .collect::<Vec<_>>();
    // Several weapon-fire loops can attach, such as the Focus Rifle's firing loop beside a heat
    // warning beep. The firing loop is the one whose attachment the weapon's `primary_firing`
    // function scales.
    if loops.len() > 1 {
        let root = behavior["root"]["datum"]
            .as_u64()
            .context("Source behavior root")?;
        let firing = nodes
            .get(&format!("{root:08X}"))
            .and_then(|node| node["data"]["object"]["attachments"].as_array())
            .into_iter()
            .flatten()
            .filter(|row| row["primary_scale"] == "primary_firing")
            .filter_map(|row| row["tag"]["datum"].as_u64())
            .collect::<BTreeSet<_>>();
        let scaled = loops
            .iter()
            .copied()
            .filter(|node| {
                node["tag"]["datum"]
                    .as_u64()
                    .is_some_and(|id| firing.contains(&id))
            })
            .collect::<Vec<_>>();
        if !scaled.is_empty() {
            return Ok(scaled);
        }
    }
    Ok(loops)
}
