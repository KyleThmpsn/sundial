//! Contact feedback belongs to the private damage profile, separately from swing events.
use super::*;
use crate::ability::banks::MeleeImpact;
use sundial::package_authoring::ability_bank::MELEE_DAMAGE_PROFILE;

pub(in crate::item) fn author(
    manager: &PackageManager,
    audio: &ImportedAudio,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<Option<MeleeImpact>> {
    let Some(key) = audio.impact_key else {
        return Ok(None);
    };
    let mut payload = stock(
        manager,
        MELEE_DAMAGE_PROFILE,
        OWNER_CLASS,
        "melee damage profile",
    )?;
    let p = Payload(payload.clone());
    let definition = p
        .pointer(0x18)
        .map_err(|e| invalid(format!("Melee damage definition: {e:#}")))?;
    let instance = p
        .pointer(0x10)
        .map_err(|e| invalid(format!("Melee damage instance: {e:#}")))?;
    if word(&payload, definition + 4)? != 0x80804B01
        || word(&payload, instance + 4)? != 0x80804B02
        || word(&payload, definition)? != MELEE_DAMAGE_PROFILE
        || word(&payload, instance)? != MELEE_DAMAGE_PROFILE
        || word(&payload, definition + 8)? as usize != instance
        || word(&payload, instance + 8)? as usize != definition
    {
        return Err(invalid(
            "Melee contact profile has an unsupported component layout",
        ));
    }
    let rows = array(&payload, definition + 0x188, 56, 0x80804B48)?;
    let first = rows
        .first()
        .ok_or_else(|| invalid("Melee contact profile has no contact sounds"))?;
    // The stock surface cues keep their media inside their banks, so each surface source
    // plays through a private copy of a contact cue, which lists its media.
    let template = native_cue(manager, word(&payload, first + 8)?)?;
    let mut used = BTreeSet::new();
    for event in &audio.impacts {
        let mask = hex(&event["mask"], "impact response mask")?;
        if !used.insert(mask) {
            return Err(invalid("Imported impact mask repeats"));
        }
        let matching = rows
            .iter()
            .copied()
            .filter(|at| word(&payload, at + 48).ok() == Some(mask))
            .collect::<Vec<_>>();
        let [at] = matching.as_slice() else {
            return Err(invalid(
                "Imported impact mask does not select one native contact sound",
            ));
        };
        let sources = list(&event["sounds"], "impact sounds")?;
        let [source] = sources.as_slice() else {
            return Err(invalid("An impact response must select one source cue"));
        };
        let native = native_cue(manager, word(&payload, at + 8)?)?;
        let sound = private_sound(
            manager,
            audio,
            source,
            &native,
            &format!("ph_{key:08x}_contact_{mask:08x}"),
            allocator,
            tags,
            &mut BTreeMap::new(),
        )?;
        put(&mut payload, at + 8, sound)?;
    }
    if used.len() != rows.len() {
        return Err(invalid(
            "Imported impact audio does not cover every native contact response",
        ));
    }
    let responses = if audio.responses.is_empty() {
        None
    } else {
        Some(responses(manager, audio, key, &template, allocator, tags)?)
    };
    let private = allocator
        .assigned_tag(tags.len(), "Imported audio", "melee contact profile")?
        .0;
    put(&mut payload, definition, private)?;
    put(&mut payload, instance, private)?;
    append(
        manager,
        allocator,
        tags,
        MELEE_DAMAGE_PROFILE,
        payload,
        crate::NewTagStorageMode::InheritTemplate,
        "melee contact profile",
    )?;
    Ok(Some(MeleeImpact {
        key,
        damage: private,
        responses,
    }))
}

/// A native cue that lists its media, as `private_sound` takes it.
fn native_cue(manager: &PackageManager, tag: u32) -> AuthoringResult<Value> {
    let cue = stock(manager, tag, SOUND_CLASS, "contact sound")?;
    let count = word(&cue, 0x40)? as usize;
    if count == 0 || cue.len() != 0x50 + count * 4 {
        return Err(invalid("Native contact cue has an unsupported media array"));
    }
    let media = (0..count)
        .map(|i| word(&cue, 0x50 + i * 4).map(|tag| format!("{tag:08X}")))
        .collect::<AuthoringResult<Vec<_>>>()?;
    Ok(serde_json::json!({
        "tag": format!("{tag:08X}"),
        "bank": format!("{:08X}", word(&cue, 0x14)?),
        "media": media,
    }))
}

/// The per-surface responses the native melee template names.
const RESPONSES: u32 = 0x80FE_E146;
const RESPONSE_CLASS: u32 = 0x8080_8BCD;

/// A private copy of the native per-surface responses, with each source sound in the rows it
/// replaces. The first branch holds the surface sounds, one cue per row, and rows the source
/// does not cover keep their stock cue.
fn responses(
    manager: &PackageManager,
    audio: &ImportedAudio,
    key: u32,
    template: &Value,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<u32> {
    let mut payload = stock(manager, RESPONSES, RESPONSE_CLASS, "melee impact responses")?;
    let groups = array(&payload, 0x80, 24, 0x8080_8BD5)?;
    let [group] = groups.as_slice() else {
        return Err(invalid("Native impact sounds have an unsupported layout"));
    };
    let mut rows = BTreeMap::new();
    for row in array(&payload, group + 8, 32, 0x8080_8BD7)? {
        let resources = array(&payload, row + 8, 4, 0x8080_0014)?;
        let [resource] = resources.as_slice() else {
            return Err(invalid("Native impact sound row has several cues"));
        };
        if rows.insert(word(&payload, row)?, *resource).is_some() {
            return Err(invalid("Native impact sound rows repeat"));
        }
    }
    let mut replaced = BTreeSet::new();
    for event in &audio.responses {
        let sources = list(&event["sounds"], "response sounds")?;
        let [source] = sources.as_slice() else {
            return Err(invalid("An impact response must select one source cue"));
        };
        let source_tag = hex(&source["tag"], "response source cue")?;
        let sound = private_sound(
            manager,
            audio,
            source,
            template,
            &format!("ph_{key:08x}_surface_{source_tag:08x}"),
            allocator,
            tags,
            &mut BTreeMap::new(),
        )?;
        for name in list(&event["rows"], "response rows")? {
            let name = hex(name, "response row")?;
            let at = rows
                .get(&name)
                .ok_or_else(|| invalid(format!("Native impact sounds lack row 0x{name:08X}")))?;
            if !replaced.insert(name) {
                return Err(invalid("Imported impact response rows repeat"));
            }
            put(&mut payload, *at, sound)?;
        }
    }
    append(
        manager,
        allocator,
        tags,
        RESPONSES,
        payload,
        crate::NewTagStorageMode::InheritTemplate,
        "melee impact responses",
    )
}
