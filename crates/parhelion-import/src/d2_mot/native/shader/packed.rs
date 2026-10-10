//! Packed rigid shaders using checked native model, view and transparent scopes.
use super::replace_once;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Serialize)]
pub struct Transform {
    pub scale: [f32; 3],
    pub offset: [f32; 3],
    pub uv: [f32; 4],
    pub rect: [usize; 4],
    pub size: [usize; 2],
    pub vertex_base: usize,
}

fn declaration(slot: usize, count: usize) -> String {
    format!("cbuffer cb{slot} : register(b{slot})\n{{\n  float4 cb{slot}[{count}];\n}}")
}

fn validate_view(text: &str) -> Result<()> {
    let body = text
        .split_once("void main(")
        .context("Packed shader entry point")?
        .1;
    for read in body.split("cb12[").skip(1) {
        let index: usize = read
            .split_once(']')
            .context("View constant index")?
            .0
            .parse()?;
        ensure!(index < 14, "Unmapped packed view input {index}");
    }
    Ok(())
}

fn scope(root: &Path, era: &str, name: &str, index: usize) -> Result<Payload> {
    let folder = root.join(format!("tfx-{era}"));
    let context: Value = serde_json::from_slice(&fs::read(folder.join("context.json"))?)?;
    let rows = context["scopes"]
        .as_array()
        .context("Renderer scopes")?
        .iter()
        .filter(|row| row["name"] == name)
        .collect::<Vec<_>>();
    ensure!(
        rows.len() == 1 && rows[0]["index"].as_u64() == Some(index as u64),
        "Missing or ambiguous {era} {name} scope"
    );
    let tag = rows[0]["tag"].as_str().context("Scope tag")?;
    ensure!(
        tag.len() == 8 && tag.bytes().all(|v| v.is_ascii_hexdigit()),
        "Invalid scope identity"
    );
    let p = Payload(fs::read(folder.join(format!("raw/{tag}.bin")))?);
    ensure!(
        p.u64(0)? as usize == p.0.len() && p.u32(16)? as usize == index,
        "Scope envelope differs"
    );
    Ok(p)
}

fn program(p: &Payload, offset: usize) -> Result<Vec<u8>> {
    Ok(p.array(offset, 1, Some(0x80800009))?
        .iter()
        .map(|&at| p.0[at])
        .collect())
}

fn validate_scope(p: &Payload, era: &str, modern: bool, name: &str) -> Result<()> {
    let vb = if modern { 0xD0 } else { 0xD8 };
    let pb = if modern { 0x48 } else { 0x40 };
    let bind = if modern { 0x68 } else { 0x78 };
    let expected = match (name, modern) {
        ("rigid_model", true) => "4c080053004b080452044b080552054b080652064b08075207",
        ("rigid_model", false) => "3e080044003d080443043d080543053d080643063d08074307",
        ("view", true) => {
            "4c021453004c020c53044a02004a02010c42004a02004a02010c040d52084b020152094b0202520a4c0208530b4b0203520f"
        }
        ("view", false) => {
            "3e021244003e020a44043c02003c02010c34003c02003c02010c040d43083d020143093e0206440a"
        }
        _ => "",
    };
    if !expected.is_empty() {
        ensure!(
            program(p, vb + 0x18)? == hex::decode(expected)?,
            "{era} {name} vertex producer differs"
        );
        let (count, slot) = if name == "view" {
            (if modern { 16 } else { 14 }, 12)
        } else {
            (8, if modern { 1 } else { 11 })
        };
        ensure!(
            p.u64(vb + 0x48)? == count && p.u32(vb + bind)? == slot,
            "{era} {name} vertex binding differs"
        );
    }
    if name == "view" {
        let expected = if modern {
            "4c021453004c020c53044c022053084a02004a02010c42004a02004a02010c040d520c4b0201520d4b0202520e"
        } else {
            "3e021244003e020a44043e021e44083c02003c02010c34003c02003c02010c040d430c3d0201430d"
        };
        ensure!(
            program(p, pb + 0x18)? == hex::decode(expected)? && p.u32(pb + bind)? == 12,
            "{era} pixel view producer differs"
        );
    }
    if name == "transparent" {
        let code = program(p, pb + 0x18)?;
        let prefix = if modern {
            "4d030f562a4d2800562b4d2801562c4d2802562d"
        } else {
            "3f0307472a3f2700472b3f2701472c3f2702472d"
        };
        let depth = if modern { "4b03005200" } else { "3d03004300" };
        ensure!(
            code.starts_with(&hex::decode(prefix)?)
                && code.windows(5).any(|v| v == hex::decode(depth).unwrap())
                && p.u64(pb + 0x48)? == 6
                && p.u32(pb + bind)? == 2,
            "{era} transparent depth or scene producer differs"
        );
    }
    if name == "transparent_advanced" {
        ensure!(
            program(p, pb + 0x18)?.is_empty() && p.u64(pb + 0x48)? >= 8 && p.u32(pb + bind)? == 8,
            "{era} transparent constant prefix differs"
        );
    }
    Ok(())
}

/// Creation requires actual scope exports from both supported package profiles.
pub struct Scopes {
    evidence: Value,
}

/// The same four-row rigid matrix is supplied at extern 8, index 0 in both profiles.
/// Accept it only with the complete inspected scope producers and binding envelopes.
pub(crate) fn rigid_matrix(root: &Path) -> Result<Value> {
    let mut evidence = Vec::new();
    for (era, modern) in [("modern", true), ("native", false)] {
        let payload = scope(root, era, "rigid_model", 2)?;
        validate_scope(&payload, era, modern, "rigid_model")?;
        evidence.push(json!({"era":era,"scope":"rigid_model","index":2,
            "sha256":hex::encode(Sha256::digest(&payload.0))}));
    }
    Ok(json!({"matrix_extern":[8,0],"scopes":evidence,
        "live_producer_execution_verified":false}))
}

impl Scopes {
    pub fn read(root: &Path) -> Result<Self> {
        let mut evidence = Vec::new();
        for (era, modern) in [("modern", true), ("native", false)] {
            for (name, index) in [
                ("rigid_model", 2),
                ("view", 1),
                ("transparent", 13),
                ("transparent_advanced", 14),
            ] {
                let p = scope(root, era, name, index)?;
                validate_scope(&p, era, modern, name)?;
                evidence.push(json!({"era":era,"scope":name,"index":index,"sha256":hex::encode(Sha256::digest(&p.0))}));
            }
        }
        Ok(Self {
            evidence: json!({"scopes":evidence,"live_producer_execution_verified":false}),
        })
    }

    pub fn receipt(&self) -> &Value {
        &self.evidence
    }

    pub fn vertex(&self, source: &str, t: &Transform) -> Result<String> {
        ensure!(
            t.scale.iter().all(|v| v.is_finite() && *v > 0.)
                && t.offset.iter().chain(&t.uv).all(|v| v.is_finite())
                && t.rect[2..].iter().chain(&t.size).all(|&v| v > 0),
            "Packed rigid transform differs"
        );
        let mut text = replace_once(source, &declaration(1, 8), &declaration(11, 8))?;
        text = replace_once(&text, &declaration(12, 15), &declaration(12, 14))?;
        text = text
            .replace("cb12[14].xyzw", "cb12[13].xyzw")
            .replace("cb12[10].xyz", "cb12[7].xyz");
        validate_view(&text)?;
        // These shader inputs have no blend indices or instance streams.
        let signature = "void main(\n  float4 v0 : POSITION0,\n  float3 v1 : NORMAL0,\n  float4 v2 : TANGENT0,\n  float2 v3 : TEXCOORD0,\n  uint v4 : SV_VERTEXID0,\n";
        text = replace_once(
            &text,
            signature,
            "void main(\n  float4 nativePosition : POSITION0,\n  float2 nativeUv : TEXCOORD0,\n  float3 nativeNormal : NORMAL0,\n  float4 nativeTangent : TANGENT0,\n  uint nativeVertex : SV_VERTEXID0,\n",
        )?;
        let scale = format!(
            "float3({:.9},{:.9},{:.9})",
            t.scale[0], t.scale[1], t.scale[2]
        );
        let offset = format!(
            "float3({:.9},{:.9},{:.9})",
            t.offset[0], t.offset[1], t.offset[2]
        );
        text = text
            .replace("cb1[4].xyz", &scale)
            .replace("cb1[5].xyz", &offset);
        text = text
            .replace("cb1[6].xyxy", "float4(1,1,1,1)")
            .replace("cb1[6].zwzw", "float4(0,0,0,0)");
        // Remaining rows are the native rigid matrix, color bound and self occlusion.
        for i in 0..8 {
            text = text.replace(&format!("cb1[{i}]"), &format!("cb11[{i}]"));
        }
        ensure!(
            !text.contains("cb1["),
            "Dynamic packed rigid model input is unsupported"
        );
        let [x, y, w, h] = t.rect;
        let [aw, ah] = t.size;
        let locals = format!(
            "uint4 bitmask, uiDest;\n  float4 v0 = float4((nativePosition.xyz * cb11[5].www + cb11[5].xyz - {offset}) / {scale},1);\n  float3 v1 = nativeNormal;\n  float4 v2 = nativeTangent;\n  float2 atlas_uv = nativeUv * float2({:.9},{:.9}) + float2({:.9},{:.9});\n  float2 v3 = (atlas_uv * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});\n  uint v4 = nativeVertex - {}u;",
            t.uv[0], t.uv[1], t.uv[2], t.uv[3], t.vertex_base
        );
        replace_once(&text, "uint4 bitmask, uiDest;", &locals)
    }

    pub fn pixel(&self, source: &str) -> Result<String> {
        // A scene-texture consumer may omit depth and lighting constants entirely.
        // If present, only the inspected native prefixes are accepted.
        for (slot, counts) in [(2, &[1][..]), (8, &[4, 8][..])] {
            ensure!(
                !source.contains(&format!("cbuffer cb{slot} "))
                    || counts
                        .iter()
                        .any(|count| source.contains(&declaration(slot, *count))),
                "Uninspected transparent constant prefix"
            );
        }
        let text = replace_once(source, &declaration(12, 15), &declaration(12, 14))?
            .replace("cb12[14].xyz", "cb12[7].xyz");
        validate_view(&text)?;
        Ok(text)
    }
}
