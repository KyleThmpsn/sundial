//! Admit complete talent cue closures into the same namespace as retained animation audio.
use super::*;
use crate::d1_roi::audio as graph_audio;

pub(super) fn program_cues(programs: &[Program]) -> Result<BTreeSet<u32>> {
    let mut cues = BTreeSet::new();
    for patch in programs
        .iter()
        .flat_map(Program::owned_programs)
        .flat_map(|program| &program.native_asset_patches)
        .flat_map(|edit| {
            edit.patches.iter().chain(
                edit.components
                    .iter()
                    .flat_map(|component| &component.patches),
            )
        })
    {
        let Some(symbol) = &patch.imported_particle else {
            continue;
        };
        let Some(suffix) = symbol.strip_prefix("audio-cue-") else {
            continue;
        };
        ensure!(
            suffix.len() == 8 && patch.expected.len() == 4 && patch.bytes.len() == 4,
            "Source audio cue patch {symbol} has an invalid identity or width"
        );
        let tag = u32::from_str_radix(suffix, 16)
            .with_context(|| format!("Source audio cue symbol {symbol}"))?;
        ensure!(
            graph_audio::cue_symbol(tag) == *symbol,
            "Source audio cue symbol {symbol} is not canonical"
        );
        cues.insert(tag);
    }
    Ok(cues)
}

fn selected(choices: &[Choice]) -> Vec<usize> {
    choices
        .iter()
        .map(|choice| choice.column)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|column| {
            let members = members(choices, column);
            if column == i64::MIN || column == i64::MAX {
                members
            } else {
                roll(choices, members, &mut Vec::new()).0
            }
        })
        .collect()
}

/// A rejected choice is removed before signature and socket indices are assigned. Repeating
/// the existing roll selection lets a later supported step fill its place without publishing
/// the audio of a choice excluded by the roll cap.
pub(super) fn admit(
    assembly: &mut Assembly<'_>,
    choices: &mut Vec<Choice>,
    unsupported: &mut Vec<Value>,
) -> Result<()> {
    let baseline = assembly
        .audio_assets
        .as_ref()
        .map(|audio| {
            serde_json::from_value::<BTreeSet<u32>>(audio["cues"].clone())
                .context("Prepared clip audio cue set")
        })
        .transpose()?
        .unwrap_or_default();
    loop {
        let mut cues = baseline.clone();
        let mut prepared = None;
        let mut rejected = None;
        for index in selected(choices) {
            let choice = &choices[index];
            let mut candidate = cues.clone();
            candidate.extend(&choice.lowered.audio_cues);
            if candidate == cues {
                continue;
            }
            match graph_audio::plan(assembly.source, &mut assembly.native, &candidate) {
                Ok(plan) => {
                    cues = candidate;
                    prepared = Some(plan);
                }
                Err(error) => {
                    rejected = Some((index, error));
                    break;
                }
            }
        }
        if let Some((index, error)) = rejected {
            let choice = choices.remove(index);
            unsupported.push(json!({"node":choice.node,"step":choice.report["step"],
                "name":choice.name,"reason":format!("Source talent audio closure: {error:#}")}));
            continue;
        }
        if let Some(prepared) = prepared {
            let mut audio = prepared.emit(&assembly.folder.join("audio-assets"))?;
            for node in audio["nodes"]
                .as_array_mut()
                .context("Talent audio nodes")?
            {
                node["file"] = json!(format!(
                    "audio-assets/{}",
                    node["file"].as_str().context("Talent audio file")?
                ));
            }
            audio["installable"] = json!(true);
            assembly.audio_assets = Some(audio);
        }
        return Ok(());
    }
}
