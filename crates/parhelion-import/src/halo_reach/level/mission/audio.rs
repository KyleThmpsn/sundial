//! Mission cue dependencies retain their source owners, frames and recording identities.
use super::Program;
use crate::halo_reach::{
    audio as recording, behavior,
    cache::{Cache, Tag},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOptions {
    /// MCC FSB banks with adjacent `.fsb.info` files, including at most one dialogue language.
    pub banks: Vec<PathBuf>,
    /// An explicitly configured or locally discovered vgmstream command line decoder.
    pub decoder: PathBuf,
}

fn supported(tag: &Tag) -> bool {
    matches!(
        tag.group.as_str(),
        "snd!" | "lsnd" | "mdlg" | "cine" | "cisc"
    )
}

struct Reader<'a> {
    cache: &'a Cache,
    pending: VecDeque<Tag>,
    edges: Vec<Value>,
    owner: u32,
}

impl Reader<'_> {
    fn link(&mut self, at: usize, role: &str) -> Result<Option<Tag>> {
        let tag = self.cache.reference(at, None)?;
        if let Some(tag) = &tag {
            ensure!(
                supported(tag),
                "Unsupported mission audio reference {}: {}",
                role,
                tag.group
            );
            self.edges
                .push(json!({"owner":self.owner,"role":role,"tag":tag,"offset":at}));
            self.pending.push_back(tag.clone());
        }
        Ok(tag)
    }

    fn looping(&mut self, at: usize) -> Result<Value> {
        let mut tracks = Vec::new();
        for row in self.cache.block(at + 0x20, 0xB0)? {
            let mut track = json!({
                "name":self.cache.string_id(self.cache.u32(row)?)?,
                "raw":hex::encode(self.cache.meta(row,0xB0)?),
                "gain_db":self.cache.f32(row+8)?,
            });
            for (name, offset) in [
                ("start", 0xC),
                ("loop", 0x1C),
                ("end", 0x2C),
                ("alternate_loop", 0x3C),
                ("alternate_end", 0x4C),
                ("transition_in", 0x5C),
                ("transition_out", 0x6C),
            ] {
                track[name] = serde_json::to_value(self.link(row + offset, name)?)?;
            }
            tracks.push(track);
        }
        let mut details = Vec::new();
        for row in self.cache.block(at + 0x2C, 0x48)? {
            details.push(json!({"sound":self.link(row+4,"detail")?,
                "raw":hex::encode(self.cache.meta(row,0x48)?)}));
        }
        Ok(json!({"raw":hex::encode(self.cache.meta(at,0x20)?),
            "tracks":tracks,"details":details}))
    }

    fn dialogue(&mut self, at: usize) -> Result<Value> {
        let mut lines = Vec::new();
        for row in self.cache.block(at, 0x14)? {
            let mut variants = Vec::new();
            for variant in self.cache.block(row + 4, 0x18)? {
                variants.push(json!({
                    "designation":self.cache.string_id(self.cache.u32(variant)?)?,
                    "sound":self.link(variant+4,"dialogue_variant")?,
                    "sound_effect":self.cache.string_id(self.cache.u32(variant+0x14)?)?,
                }));
            }
            lines.push(json!({"name":self.cache.string_id(self.cache.u32(row)?)?,
                "default_sound_effect":self.cache.string_id(self.cache.u32(row+0x10)?)?,
                "variants":variants}));
        }
        Ok(json!({"lines":lines}))
    }

    fn cinematic(&mut self, at: usize) -> Result<Value> {
        let mut scenes = Vec::new();
        for row in self.cache.block(at + 0x70, 0x10)? {
            scenes.push(self.link(row, "scene")?);
        }
        Ok(
            json!({"name":self.cache.string_id(self.cache.u32(at+0x28)?)?,
            "channel":self.cache.u16(at+0x2C)?,"scenes":scenes,
            "movie":self.cache.reference(at+0x4C,None)?}),
        )
    }

    fn scene(&mut self, at: usize) -> Result<Value> {
        let mut shots = Vec::new();
        for row in self.cache.block(at + 0x2C, 0xD0)? {
            let mut dialogue = Vec::new();
            for cue in self.cache.block(row + 0x40, 0x3C)? {
                dialogue.push(json!({
                    "sound":self.link(cue,"cinematic_dialogue")?,
                    "female_sound":self.link(cue+0x10,"cinematic_female_dialogue")?,
                    "frame":self.cache.i32(cue+0x20)?,
                    "scale":self.cache.f32(cue+0x24)?,
                    "lipsync_actor":self.cache.string_id(self.cache.u32(cue+0x28)?)?,
                    "sound_effect":self.cache.string_id(self.cache.u32(cue+0x2C)?)?,
                    "subtitle":self.cache.string_id(self.cache.u32(cue+0x30)?)?,
                    "female_subtitle":self.cache.string_id(self.cache.u32(cue+0x34)?)?,
                    "character":self.cache.string_id(self.cache.u32(cue+0x38)?)?,
                }));
            }
            let mut music = Vec::new();
            for cue in self.cache.block(row + 0x4C, 0x18)? {
                music.push(json!({"flags":self.cache.u32(cue)?,
                    "sound":self.link(cue+4,"cinematic_music_foley")?,
                    "frame":self.cache.i32(cue+0x14)?}));
            }
            shots.push(json!({"frames":self.cache.i32(row+0xA8)?,
                "dialogue":dialogue,"music":music}));
        }
        Ok(json!({"name":self.cache.string_id(self.cache.u32(at)?)?,
            "anchor":self.cache.string_id(self.cache.u32(at+4)?)?,"shots":shots}))
    }
}

pub(super) fn read(cache: &Cache, program: &Program) -> Result<Value> {
    let mut roots = BTreeMap::new();
    for tag in program
        .tag_references
        .iter()
        .flatten()
        .chain(
            program
                .bindings
                .tags
                .iter()
                .filter_map(|binding| binding.tag.as_ref()),
        )
        .filter(|tag| supported(tag))
    {
        roots.insert(tag.datum, tag.clone());
    }
    let at = program.scenario.address()?;
    for (offset, stride, field) in [(0x670, 0x10, 0), (0x77C, 0x14, 4)] {
        for row in cache.block(at + offset, stride)? {
            if let Some(tag) = cache.reference(row + field, None)? {
                ensure!(
                    supported(&tag),
                    "Unsupported scenario audio root {}",
                    tag.group
                );
                roots.insert(tag.datum, tag);
            }
        }
    }
    let mut reader = Reader {
        cache,
        pending: roots.values().cloned().collect(),
        edges: Vec::new(),
        owner: 0,
    };
    let mut nodes = BTreeMap::new();
    while let Some(tag) = reader.pending.pop_front() {
        crate::cancellation::check()?;
        let key = format!("{:08X}", tag.datum);
        if nodes.contains_key(&key) {
            continue;
        }
        reader.owner = tag.datum;
        let at = tag.address()?;
        let data = match tag.group.as_str() {
            "snd!" => behavior::sound(cache, &tag)?,
            "lsnd" => reader.looping(at)?,
            "mdlg" => reader.dialogue(at)?,
            "cine" => reader.cinematic(at)?,
            "cisc" => reader.scene(at)?,
            _ => unreachable!(),
        };
        nodes.insert(key, json!({"tag":tag,"data":data}));
    }
    Ok(
        json!({"roots":roots.values().collect::<Vec<_>>(),"nodes":nodes,
        "edges":reader.edges,"playback":"unsupported",
        "limits":[
            "Mission script sound literals, scenario dialogue, cinematic dialogue and music cues are preserved.",
            "Source frame numbers and loop metadata are not a target playback schedule.",
            "Bink movie audio, AI combat dialogue and sounds reached through arbitrary object or effect graphs are not included."
        ]}),
    )
}

pub(in crate::halo_reach::level) fn export(
    program: &Program,
    options: &AudioOptions,
    pages: &mut Pages,
    output: &Path,
) -> Result<Value> {
    let mut banks = Vec::new();
    let mut unique = BTreeSet::new();
    for path in &options.banks {
        let path = path
            .canonicalize()
            .with_context(|| format!("MCC audio bank {}", path.display()))?;
        ensure!(
            unique.insert(path.clone()),
            "MCC audio bank was supplied twice"
        );
        banks.push(path);
    }
    let bank = banks
        .first()
        .context("Mission audio requires at least one MCC FSB bank")?
        .clone();
    let options = recording::Options {
        bank,
        additional_banks: banks.into_iter().skip(1).collect(),
        record_missing_samples: false,
        fire_bus: None,
        firing: None,
        decoder: options.decoder.clone(),
    };
    pages.record(&options.decoder.canonicalize()?)?;
    let report = recording::export(&options, &program.audio, pages, output)?;
    fs::write(
        output.join("cues.json"),
        serde_json::to_vec_pretty(&program.audio)?,
    )?;
    Ok(
        json!({"status":"decoded","file":"audio/audio.json","cues":"audio/cues.json",
        "samples":report["samples"].as_array().map(Vec::len),"playback":"unsupported"}),
    )
}
