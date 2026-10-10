//! Complete source banks and media, using explicit allocated media references.
use super::*;
use crate::d2_mot::audio::{
    bank::{Bank, Namespace},
    cue::{Bindings, Cue, Settings, empty_auxiliary},
    prepare_sounds,
};

fn hash(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("sound source tag")?,
        16,
    )?)
}

fn raw(native: &Reader, tag: u32) -> Result<()> {
    ensure!(
        native
            .manager
            .get_entry(tiger_pkg::TagHash(tag))
            .is_some_and(|entry| entry.file_type == 26),
        "native sound template is not raw audio"
    );
    Ok(())
}

fn settings(
    source: &mut Reader,
    native: &mut Reader,
    source_tag: u32,
    fallback: u32,
) -> Result<(u32, Payload, bool)> {
    let original = source.tag(source_tag, Some(0x80808A41))?;
    ensure!(
        original.0.len() == 20 && original.u32(0)? == 0,
        "source sound settings need another layout"
    );
    for tag in native.classes(0x80808E68) {
        let payload = native.tag(tag, Some(0x80808E68))?;
        if payload.0.len() == 24 && payload.u64(0)? == 24 && payload.0[8..] == original.0[4..] {
            return Ok((tag, (*payload).clone(), true));
        }
    }
    let payload = native.tag(fallback, Some(0x80808E68))?;
    Ok((fallback, (*payload).clone(), false))
}

pub(super) fn convert(
    source: &mut Reader,
    native: &mut Reader,
    assets: &mut Assets,
    sounds: &BTreeSet<u32>,
    template: u32,
    request: &Request<'_>,
) -> Result<(BTreeMap<u32, u32>, Value)> {
    if sounds.is_empty() {
        return Ok((BTreeMap::new(), json!({"source_audio":"no source cues"})));
    }
    let prepared = prepare_sounds(
        request.source_packages,
        request.native_packages,
        request.directory,
        &sounds.iter().copied().collect::<Vec<_>>(),
    )?;
    let native_cue = native.tag(template, Some(0x80809802))?;
    let bank_template = native_cue.u32(0x14)?;
    raw(native, bank_template)?;
    let media_template = native_cue
        .array(0x18, 4, Some(0x80800014))?
        .first()
        .copied()
        .context("native audio media template")?;
    let media_template = native_cue.u32(media_template)?;
    raw(native, media_template)?;
    let mut bindings = Bindings {
        banks: BTreeMap::new(),
        settings: BTreeMap::new(),
        media: BTreeMap::new(),
    };
    let mut media_ids = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut media_bytes = BTreeMap::new();
    let mut media_symbols = BTreeMap::new();
    for row in prepared["transcoded_media"]
        .as_array()
        .context("transcoded source audio")?
    {
        let tag = hash(&row["source_tag"])?;
        let id = hash(&row["media_id"])?;
        let bytes = std::fs::read(
            request
                .directory
                .join(row["file"].as_str().context("PCM file")?),
        )?;
        let private = if let Some(previous) = media_ids.get(&id) {
            ensure!(
                media_bytes.get(&id) == Some(&bytes),
                "source media identity names different samples"
            );
            *previous
        } else {
            let symbol = format!("attachment-audio-media-{tag:08X}");
            let private = assets.reserve(native, symbol.clone())?;
            sizes.insert(id, u32::try_from(bytes.len())?);
            media_bytes.insert(id, bytes.clone());
            assets.audio(private, media_template, bytes, false, Vec::new())?;
            media_ids.insert(id, private);
            media_symbols.insert(id, symbol);
            private
        };
        // Capture the original medium in this candidate's source receipt as well.
        source.tag(tag, None)?;
        bindings.media.insert(tag, private);
    }
    let mut banks = Vec::new();
    let mut bank_tags = Vec::new();
    for row in prepared["converted_banks"]
        .as_array()
        .context("converted source banks")?
    {
        let tag = hash(&row["source_tag"])?;
        let bank: Bank = serde_json::from_slice(&std::fs::read(
            request
                .directory
                .join(row["file"].as_str().context("bank file")?),
        )?)?;
        source.tag(tag, None)?;
        bank_tags.push(tag);
        banks.push(bank);
    }
    let namespace = Namespace::new(
        &format!("{}/audio/{:08X}", request.namespace, request.source_tag),
        &banks,
    )?;
    for (tag, bank) in bank_tags.into_iter().zip(&banks) {
        let private = assets.reserve(native, format!("attachment-audio-bank-{tag:08X}"))?;
        let mut bytes = namespace.instantiate(bank, &media_ids, &sizes)?;
        let mut patches = Vec::new();
        for (offset, id) in bank.media_fields() {
            let symbol = media_symbols
                .get(&id)
                .context("bank medium is outside cue closure")?;
            bytes
                .get_mut(offset..offset + 4)
                .context("bank media offset")?
                .copy_from_slice(&u32::MAX.to_le_bytes());
            patches.push((offset, symbol.clone()));
        }
        assets.audio(private, bank_template, bytes, true, patches)?;
        bindings.banks.insert(tag, private);
    }
    let mut converted = BTreeMap::new();
    let mut reports = Vec::new();
    for &tag in sounds {
        let original = source.tag(tag, Some(0x80809738))?;
        let cue = Cue::read(&original)?;
        let matched = if let std::collections::btree_map::Entry::Vacant(entry) =
            bindings.settings.entry(cue.settings)
        {
            let (template, payload, matched) =
                settings(source, native, cue.settings, native_cue.u32(0x10)?)?;
            let private = assets.reserve(
                native,
                format!("attachment-audio-settings-{:08X}", cue.settings),
            )?;
            assets.push(private, template, payload, None)?;
            entry.insert(Settings {
                tag: private,
                class: 0x80808E68,
            });
            matched
        } else {
            false
        };
        if let std::collections::btree_map::Entry::Vacant(entry) =
            bindings.settings.entry(cue.auxiliary)
        {
            let original = source.tag(cue.auxiliary, Some(0x8080BCA8))?;
            let bytes = empty_auxiliary(&original)?;
            let template = native_cue.u32(0x28)?;
            native.tag(template, Some(0x80808E69))?;
            let private = assets.reserve(
                native,
                format!("attachment-audio-auxiliary-{:08X}", cue.auxiliary),
            )?;
            assets.push(private, template, Payload(bytes), None)?;
            entry.insert(Settings {
                tag: private,
                class: 0x80808E69,
            });
        }
        let bytes = cue.emit(&namespace, &bindings)?;
        let private = assets.reserve(native, format!("attachment-audio-cue-{tag:08X}"))?;
        assets.push(private, template, Payload(bytes), None)?;
        converted.insert(tag, private);
        reports.push(
            json!({"source_cue":tag,"source_bank":cue.bank,"media_count":cue.media.len(),
            "source_settings_matched":matched,"difference":if matched { Value::Null } else {
                json!("The source bank and samples use a native acoustic settings profile.") }}),
        );
    }
    Ok((
        converted,
        json!({"source_audio":"translated source banks and PCM media","cues":reports,
        "bank_count":banks.len(),"media_count":media_ids.len(),"gameplay_verified":false}),
    ))
}
