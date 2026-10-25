//! Bind supplemental clips through named layers and playback drivers.
use super::*;

struct Entry {
    clip: u32,
    driver: u32,
}

fn functions(reader: &mut Reader, rig: &Value, modern: bool) -> Result<Vec<u32>> {
    let (owner, _, _) = poses::controller(reader, rig, modern)?;
    let owner = reader.tag(owner, None)?;
    let tag = owner.u32(owner.pointer(24)? + if modern { 0x140 } else { 0x110 })?;
    let graph = reader.tag(tag, None)?;
    graph
        .array(8, 24, Some(if modern { 0x80802811 } else { 0x80803650 }))?
        .into_iter()
        .map(|at| graph.u32(at))
        .collect()
}

fn entries(
    p: &Payload,
    row: usize,
    modern: bool,
    clips: &[(u32, u32)],
    functions: &[u32],
) -> Result<Vec<Entry>> {
    let mut result = Vec::new();
    for choice in p.array(row, 16, Some(if modern { 0x80802903 } else { 0x8080372E }))? {
        let rows = p.array(
            choice,
            if modern { 80 } else { 64 },
            Some(if modern { 0x80802905 } else { 0x80803730 }),
        )?;
        let [at] = rows.as_slice() else { continue };
        let at = *at;
        if p.u64(at + if modern { 0 } else { 16 })? != 0 {
            continue;
        }
        let weighted = p.array(
            at + if modern { 16 } else { 0 },
            8,
            Some(if modern { 0x8080290C } else { 0x80803737 }),
        )?;
        let [weighted] = weighted.as_slice() else {
            continue;
        };
        // Only the named-function weight driver, with fixed playback and no
        // time driver. More complex choices need their own control translation.
        if p.f32(weighted + 4)? != 1.0
            || p.u32(at + 32)? != 2
            || p.u32(at + 36)? != 0
            || p.u16(at + 42)? != 1
            || p.u32(at + 44)? != 0
            || p.f32(at + 48)? != 1.0
        {
            continue;
        }
        let fixed = if modern {
            p.u16(at + 52)? == 0
                && p.u32(at + 54)? == 0xFFFF
                && p.u16(at + 58)? == 0
                && p.u32(at + 60)? == 1
        } else {
            p.u32(at + 56)? == 0xFFFF && p.u32(at + 60)? == 0x01000001
        };
        if !fixed {
            continue;
        }
        let driver = *functions
            .get(usize::from(p.u16(at + 40)?))
            .context("motion weight driver exceeds function table")?;
        // A name repeated in the function table does not identify a driver.
        if functions.iter().filter(|&&name| name == driver).count() != 1 {
            continue;
        }
        let clip = clips
            .get(usize::try_from(p.u32(*weighted)?)?)
            .context("motion clip exceeds animation bank")?
            .0;
        result.push(Entry { clip, driver });
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bindings(
    sr: &mut Reader,
    nr: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    source_clips: &[(u32, u32)],
    native_clips: &[(u32, u32)],
) -> Result<Vec<(u32, u32, u32)>> {
    let (_, _, source) = poses::controller(sr, source_rig, true)?;
    let (_, _, native) = poses::controller(nr, native_rig, false)?;
    let sf = functions(sr, source_rig, true)?;
    let nf = functions(nr, native_rig, false)?;
    let lookup = first_person(native_rig, NATIVE_LOOKUP)?.context("native motion lookup")?;
    let (tag, _) = super::bank(nr, lookup.lookup_owner, false)?;
    let bank = nr.tag(tag, Some(0x808036F6))?;
    let action_slots = bank
        .array(0x68, 32, Some(0x80809002))?
        .into_iter()
        .map(|at| bank.u16(at + 24))
        .collect::<Result<BTreeSet<_>>>()?;
    let action_clips = action_slots
        .into_iter()
        .map(|slot| {
            native_clips
                .get(usize::from(slot))
                .map(|v| v.0)
                .context("motion action slot exceeds bank")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut native_layers = BTreeMap::new();
    for row in native.array(8, 24, Some(0x80803724))? {
        ensure!(
            native_layers.insert(native.u32(row + 16)?, row).is_none(),
            "motion layer names repeat"
        );
    }
    let mut bindings = BTreeMap::new();
    for row in source.array(8, 32, Some(0x808028F9))? {
        let name = source.u32(row + 16)?;
        let Some(&to) = native_layers.get(&name) else {
            continue;
        };
        let from = entries(&source, row, true, source_clips, &sf)?;
        let to = entries(&native, to, false, native_clips, &nf)?;
        for target in &to {
            let matches = from
                .iter()
                .filter(|entry| entry.driver == target.driver)
                .collect::<Vec<_>>();
            let [input] = matches.as_slice() else {
                continue;
            };
            if action_clips.contains(&target.clip) {
                continue;
            }
            let source_clip = sr.tag(input.clip, Some(0x80808BE0))?;
            let native_clip = nr.tag(target.clip, Some(0x80808F49))?;
            // Keep the native baked duration valid, and leave eventful motions
            // to the event translator rather than borrowing carrier feedback.
            if source_clip.u16(0x140)? <= 2
                || source_clip.u16(0x140)? != native_clip.u16(0x13C)?
                || source_clip.u64(0x160)? != 0
                || native_clip.u64(0x160)? != 0
            {
                continue;
            }
            if let Some((old, _)) = bindings.insert(target.clip, (input.clip, name)) {
                ensure!(old == input.clip, "supplemental motion aliases disagree");
            }
        }
    }
    Ok(bindings
        .into_iter()
        .map(|(n, (s, layer))| (n, s, layer))
        .collect())
}

/// Refresh supplemental clips without reauthoring established action routes.
pub fn refresh(
    sr: &mut Reader,
    nr: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    calibration: &Value,
    directory: &Path,
    graph: &mut Value,
) -> Result<()> {
    let source = first_person(source_rig, SOURCE_LOOKUP)?.context("source motion rig")?;
    let native = first_person(native_rig, NATIVE_LOOKUP)?.context("native motion rig")?;
    let (_, source_clips) = super::bank(sr, source.lookup_owner, true)?;
    let (_, native_clips) = super::bank(nr, native.lookup_owner, false)?;
    let mut pairs = clip_pairs(sr, nr, &native_clips, &source_clips)?;
    pairs.extend(calibration_clips(sr, nr, &native_clips, Some(calibration))?);
    let slots = clips::slots::Table::derive(pairs.iter().map(|p| (p.3.as_ref(), p.4.as_ref())))?;
    let bindings = bindings(sr, nr, source_rig, native_rig, &source_clips, &native_clips)?;
    let fp = &mut graph["animation"]["first_person"];
    let mut converted = Vec::new();
    // Validate and convert everything before writing any clip files.
    for &(native, source, layer) in &bindings {
        let original = sr.tag(source, Some(0x80808BE0))?;
        let (mut payload, report) = clips::convert(&original.0)?;
        slots.apply(&mut payload)?;
        if let Some(old) = fp["clips"]
            .as_array()
            .context("motion clips")?
            .iter()
            .find(|c| c["native"].as_u64() == Some(u64::from(native)))
        {
            ensure!(
                old["source"].as_u64() == Some(u64::from(source)),
                "existing source clip conflicts with supplemental motion"
            );
        }
        let file = format!("animation/clip-{native:08X}.bin");
        let row = json!({"native":native,"source":source,"name":original.u32(0x120)?,
            "file":file,"streams":report["streams"],"events":report["events"],
            "motion_layer":layer});
        converted.push((file, payload, row));
    }
    let clips = fp["clips"].as_array_mut().context("motion clips")?;
    for (file, payload, row) in converted {
        fs::write(directory.join(file), payload.0)?;
        clips.retain(|old| old["native"] != row["native"]);
        clips.push(row);
    }
    fp["motion_bindings"] = json!(
        bindings
            .iter()
            .map(|&(n, s, l)| json!({"native":n,"source":s,"layer":l}))
            .collect::<Vec<_>>()
    );
    Ok(())
}
