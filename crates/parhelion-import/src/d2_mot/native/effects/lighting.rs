//! Lower the modern forward-lighting resource contract to native environment light.
//! Material equations stay intact. Native SH atlases replace clipmap sampling and
//! native hemisphere reflection replaces the two renderer-owned local probe volumes.
use super::*;
use crate::d2_mot::native::shader::replace_once;
use std::collections::BTreeSet;

type Components = BTreeSet<(String, char)>;

fn references(text: &str) -> Components {
    let bytes = text.as_bytes();
    let mut found = Components::new();
    for at in 0..bytes.len() {
        if !matches!(bytes[at], b'r' | b'o')
            || (at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_'))
        {
            continue;
        }
        let mut end = at + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == at + 1 || bytes.get(end) != Some(&b'.') {
            continue;
        }
        let name = &text[at..end];
        end += 1;
        while end < bytes.len() && b"xyzw".contains(&bytes[end]) {
            found.insert((name.to_owned(), bytes[end] as char));
            end += 1;
        }
    }
    found
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (left, right) = line.trim().strip_suffix(';')?.split_once(" = ")?;
    if !left.starts_with('r') && !left.starts_with('o') {
        return None;
    }
    (!references(left).is_empty() && !left.contains(' ')).then_some((left, right))
}

fn live(lines: &[String]) -> Components {
    let mut result = Components::new();
    for line in lines.iter().rev() {
        if let Some((left, right)) = assignment(line) {
            let writes = references(left);
            if left.starts_with('o') || !writes.is_disjoint(&result) {
                result.retain(|v| !writes.contains(v));
                result.extend(references(right));
            }
        } else {
            result.extend(references(line));
        }
    }
    result
}

fn replace_region(
    lines: &mut Vec<String>,
    start: usize,
    end: usize,
    replacement: Vec<String>,
) -> Result<()> {
    ensure!(
        start < end && end <= lines.len(),
        "forward-lighting region extent differs"
    );
    let needed = live(&lines[end..]);
    let written = lines[start..end]
        .iter()
        .filter_map(|l| assignment(l))
        .flat_map(|(left, _)| references(left))
        .collect::<Components>();
    let provided = replacement
        .iter()
        .filter_map(|l| assignment(l))
        .flat_map(|(left, _)| references(left))
        .collect::<Components>();
    ensure!(
        written.intersection(&needed).all(|v| provided.contains(v)),
        "forward-lighting region has additional live outputs"
    );
    lines.splice(start..end, replacement);
    Ok(())
}

fn unique(lines: &[String], needle: &str) -> Result<usize> {
    let found = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "forward-lighting contract for {needle} is ambiguous"
    );
    Ok(found[0])
}

fn probes(lines: &mut Vec<String>) -> Result<()> {
    let start = unique(lines, "= cb8[25].w *")?;
    let first = unique(lines, "= cb8[28].w *")?;
    let second = unique(lines, "= cb8[14].w *")?;
    ensure!(
        start < first && first + 2 < second && second + 2 < lines.len(),
        "reflection volume order differs"
    );
    let (_, first_blend) = assignment(&lines[first + 2]).context("first reflection blend")?;
    let (destination, second_blend) =
        assignment(&lines[second + 2]).context("second reflection blend")?;
    let base = first_blend
        .split_once(" + ")
        .context("native hemisphere reflection")?
        .1;
    ensure!(
        first_blend.contains(" * ")
            && second_blend.contains(" * ")
            && second_blend.contains(" + ")
            && references(base).len() == 3,
        "reflection volume blend differs"
    );
    let replacement = vec![format!("  {destination} = {base};")];
    replace_region(lines, start, second + 3, replacement)
}

fn irradiance(lines: &mut Vec<String>) -> Result<()> {
    let start = unique(lines, "= -cb3[0].xyz +")?;
    let samples = [16, 17, 18].map(|slot| {
        lines
            .iter()
            .position(|l| l.contains(&format!("= t{slot}.SampleLevel(")))
            .context("clipmap coefficient sample")
    });
    let samples = samples.into_iter().collect::<Result<Vec<_>>>()?;
    let coefficients = samples
        .iter()
        .map(|i| {
            assignment(&lines[*i])
                .map(|v| v.0.to_owned())
                .context("clipmap coefficient register")
        })
        .collect::<Result<Vec<_>>>()?;
    let shadow = unique(lines, "= t19.SampleLevel(")?;
    let dot = lines
        .iter()
        .position(|l| l.contains(&format!("= dot({},", coefficients[0])))
        .context("irradiance dot product")?;
    ensure!(
        start < samples[0]
            && samples[0] < samples[1]
            && samples[1] < samples[2]
            && samples[2] < dot
            && shadow > dot
            && (shadow - dot + 1).is_multiple_of(5)
            && shadow + 2 < lines.len(),
        "irradiance equation order differs"
    );
    let shadow_result = assignment(&lines[shadow + 2])
        .context("shadow blend destination")?
        .0;
    for evaluation in lines[dot - 1..shadow].chunks_exact(5) {
        let (red, dot_expression) =
            assignment(&evaluation[1]).context("irradiance red evaluation")?;
        let normal = dot_expression
            .strip_prefix(&format!("dot({}, ", coefficients[0]))
            .and_then(|v| v.strip_suffix(')'))
            .context("irradiance normal vector")?;
        let color = red
            .strip_suffix(".x")
            .context("irradiance red destination")?;
        ensure!(
            evaluation[0].trim()
                == format!(
                    "{}.w = 1;",
                    normal
                        .strip_suffix(".xyzw")
                        .context("irradiance normal layout")?
                )
                && evaluation[2].trim()
                    == format!("{color}.y = dot({}, {normal});", coefficients[1])
                && evaluation[3].trim()
                    == format!("{color}.z = dot({}, {normal});", coefficients[2])
                && evaluation[4].trim()
                    == format!("{color}.xyz = max(float3(0,0,0), {color}.xyz);"),
            "irradiance coefficient evaluation differs"
        );
    }
    let mut replacement = coefficients.iter().enumerate().map(|(i, name)| format!("  {name} = t{}.SampleLevel(s5_s, cb8[4].zw * float2(1,0.5) + float2(0,0.5), 0).xyzw;",28+i)).collect::<Vec<_>>();
    // Preserve the source normal and RGB spherical-harmonic evaluation.
    replacement.extend_from_slice(&lines[dot - 1..shadow]);
    replacement.push(format!(
        "  {shadow_result} = t31.SampleLevel(s4_s, cb8[4].zw, 0).x;"
    ));
    replace_region(lines, start, shadow + 3, replacement)
}

pub(super) fn adapt(text: &str) -> Result<(String, bool)> {
    let count = inputs::cb_count(text, 8)?;
    if inputs::cb_count(text, 3)? != Some(16) || !matches!(count, Some(8 | 36)) {
        return Ok((text.to_owned(), false));
    }
    ensure!(
        (27..=31).all(|slot| !text.contains(&format!("register(t{slot})"))),
        "native irradiance lookup slots are occupied"
    );
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    if count == Some(36) {
        probes(&mut lines)?;
    }
    irradiance(&mut lines)?;
    let mut result = lines.join("\n");
    // Only straight-line equations and early discard are accepted by the
    // component-liveness proof above. The clipmap branch has been replaced.
    let body = result
        .split_once("void main(")
        .context("lighting shader entry")?
        .1;
    ensure!(
        !body.contains("else")
            && !body
                .lines()
                .any(|l| l.contains("if (") && !l.contains("discard;"))
            && !body.contains("for (")
            && !body.contains("while ("),
        "forward-lighting shader has additional control flow"
    );
    ensure!(
        inputs::reads(&result, 3).is_some_and(|r| r.is_empty())
            && inputs::reads(&result, 8).is_some_and(|r| r.iter().all(|i| *i < 8)),
        "forward-lighting shader retains modern scope inputs"
    );
    result = replace_once(&result, &inputs::cb_decl(3, 16), "")?;
    if count == Some(36) {
        result = replace_once(&result, &inputs::cb_decl(8, 36), &inputs::cb_decl(8, 8))?;
    }
    for slot in [16, 17, 18, 19, 24, 25]
        .into_iter()
        .filter(|slot| *slot < 24 || count == Some(36))
    {
        ensure!(
            !result.contains(&format!("t{slot}.")),
            "forward-lighting shader retains a modern volume read"
        );
        result = replace_once(
            &result,
            &format!(
                "{}<float4> t{slot} : register(t{slot});",
                if slot >= 24 {
                    "TextureCube"
                } else {
                    "Texture3D"
                }
            ),
            "",
        )?;
    }
    for slot in 28..=31 {
        result = format!("Texture2D<float4> t{slot} : register(t{slot});\n{result}");
    }
    if inputs::slots(&result, 't')?.contains(&11) {
        // Resolve native RGB and directional fog into the pre-lit color used
        // by modern shaders. Alpha is zero because its directional term is
        // already included in RGB, not another term for the source to relight.
        for slot in [11, 13] {
            ensure!(
                result.matches(&format!("t{slot}.")).count() == 1
                    && result.contains(&format!("t{slot}.Sample(")),
                "forward fog sampling contract differs"
            );
            result = replace_once(
                &result,
                &format!("Texture2D<float4> t{slot} : register(t{slot});"),
                "",
            )?;
            result = result.replace(&format!("t{slot}.Sample("), "native_fog(");
        }
        result = format!("Texture2D<float4> t27 : register(t27);\n{result}");
        result = replace_once(
            &result,
            "void main(\n",
            "float4 native_fog(SamplerState fog_sampler, float2 uv) { float4 fog = t27.Sample(fog_sampler, uv); return float4(cb8[5].xyz * fog.xyz + cb8[6].xyz * fog.w, 0); }\nvoid main(\n",
        )?;
    }
    Ok((result, true))
}

pub(super) fn bindings(refs: &Path) -> Result<Vec<u8>> {
    let context = load(&refs.join("tfx-native/context.json"))?;
    let scope = context["scopes"]
        .as_array()
        .context("native renderer scopes")?
        .iter()
        .find(|s| s["name"] == "transparent")
        .context("native transparent scope")?;
    let scope = Payload(fs::read(refs.join(format!(
        "tfx-native/raw/{}.bin",
        scope["tag"].as_str().context("native transparent tag")?
    )))?);
    let code = array_bytes(&scope, 0x58, 1)?;
    let mut result = Vec::new();
    for (resource, native_slot, target_slot) in [
        (0, 11, 27),
        (1, 12, 28),
        (2, 13, 29),
        (3, 14, 30),
        (4, 15, 31),
    ] {
        let contract = [0x3F, 0x27, resource, 0x47, 0x20 | native_slot];
        ensure!(
            code.windows(5).filter(|w| *w == contract).count() == 1,
            "native irradiance binding contract differs"
        );
        result.extend([0x3F, 0x27, resource, 0x47, 0x20 | target_slot]);
    }
    Ok(result)
}

fn refraction_contract(modern: &[u8], native: &[u8]) -> Result<()> {
    // Both shaders sample the resolved scene color in screen coordinates,
    // apply exposure, then clamp luminance before the same distortion blend.
    ensure!(
        modern
            .windows(5)
            .filter(|w| *w == [0x4D, 0x28, 11, 0x56, 0x36])
            .count()
            == 1
            && native
                .windows(5)
                .filter(|w| *w == [0x3F, 0x27, 7, 0x47, 0x32])
                .count()
                == 1,
        "native screen-refraction contract differs"
    );
    Ok(())
}

pub(super) fn check_refraction(refs: &Path) -> Result<()> {
    let mut code = Vec::new();
    for (era, offset) in [("modern", 0x60), ("native", 0x58)] {
        let context = load(&refs.join(format!("tfx-{era}/context.json")))?;
        let row = context["scopes"]
            .as_array()
            .context("renderer scopes")?
            .iter()
            .find(|s| s["name"] == "transparent")
            .context("transparent scope")?;
        let data = Payload(fs::read(refs.join(format!(
            "tfx-{era}/raw/{}.bin",
            row["tag"].as_str().context("scope tag")?
        )))?);
        code.push(array_bytes(&data, offset, 1)?);
    }
    refraction_contract(&code[0], &code[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refraction_requires_both_exact_renderer_bindings() {
        let modern = [0x4D, 0x28, 11, 0x56, 0x36];
        let native = [0x3F, 0x27, 7, 0x47, 0x32];
        refraction_contract(&modern, &native).unwrap();
        assert!(refraction_contract(&[0x4D, 0x28, 10, 0x56, 0x36], &native).is_err());
        assert!(refraction_contract(&modern, &[0x3F, 0x27, 7, 0x47, 0x33]).is_err());
    }
    #[test]
    fn region_replacement_rejects_lost_outputs_and_preserves_components() {
        let mut lines = ["r9.xyz = r1.xyz;", "r9.w = r2.x;", "o0.xyzw = r9.xyzw;"]
            .map(str::to_owned)
            .to_vec();
        assert!(replace_region(&mut lines, 0, 2, vec!["r9.xyz = r3.xyz;".into()]).is_err());
        replace_region(&mut lines, 0, 1, vec!["r9.xyz = r3.xyz;".into()]).unwrap();
        assert_eq!(
            live(&lines),
            Components::from([
                ("r3".into(), 'x'),
                ("r3".into(), 'y'),
                ("r3".into(), 'z'),
                ("r2".into(), 'x')
            ])
        );
    }
    #[test]
    fn unrelated_or_incomplete_contracts_are_not_lowered() {
        assert!(!adapt("void main() {}").unwrap().1);
        assert!(
            adapt(&format!(
                "{}\n{}\nvoid main() {{}}",
                inputs::cb_decl(3, 16),
                inputs::cb_decl(8, 36)
            ))
            .is_err()
        );
    }

    #[test]
    fn native_lighting_preserves_material_outputs_across_register_allocations() {
        let mut fixture = format!("{}\n{}\n", inputs::cb_decl(3, 16), inputs::cb_decl(8, 36));
        for slot in [16, 17, 18, 19, 24, 25] {
            fixture.push_str(&format!(
                "{}<float4> t{slot} : register(t{slot});\n",
                if slot < 24 {
                    "Texture3D"
                } else {
                    "TextureCube"
                }
            ));
        }
        fixture.push_str("SamplerState s4_s : register(s4);\nSamplerState s5_s : register(s5);\nvoid main(\nfloat4 v0 : SV_POSITION, out float4 o0 : SV_TARGET) {\nfloat4 r0,r1,r2,r4,r5,r6,r7,r9,r10,r11,r12;\n");
        fixture.push_str("r0.xyz = float3(0,0,1);\nr1.w = 1;\nr2.x = 0.5;\n");
        fixture.push_str("r5.xyz = -cb3[0].xyz + v0.xyz;\nr10.xyzw = t16.SampleLevel(s5_s, r5.xyz, 0).xyzw;\nr11.xyzw = t17.SampleLevel(s5_s, r5.xyz, 0).xyzw;\nr12.xyzw = t18.SampleLevel(s5_s, r5.xyz, 0).xyzw;\nr0.w = 1;\nr9.x = dot(r10.xyzw, r0.xyzw);\nr9.y = dot(r11.xyzw, r0.xyzw);\nr9.z = dot(r12.xyzw, r0.xyzw);\nr9.xyz = max(float3(0,0,0), r9.xyz);\nr0.w = t19.SampleLevel(s4_s, r5.xyz, 0).x;\nr0.w = -1 + r0.w;\nr1.w = r1.w * r0.w + 1;\n");
        fixture.push_str("r2.yzw = float3(0.25,0.5,0.75);\nr4.xzw = r2.yzw;\nr6.y = cb8[25].w * r2.x;\nr6.xyz = t25.SampleLevel(s5_s, v0.xyz, r6.y).xyz;\nr7.w = 1;\nr7.w = cb8[28].w * r7.w;\nr2.yzw = -r2.yzw + r6.xyz;\nr2.yzw = r7.www * r2.yzw + r4.xzw;\nr2.x = cb8[11].w * r2.x;\nr4.xyzw = t24.SampleLevel(s5_s, v0.xyz, r2.x).xyzw;\nr2.x = cb8[14].w * r2.x;\nr4.xyz = r4.xyz + -r2.yzw;\nr2.xyz = r2.xxx * r4.xyz + r2.yzw;\no0.xyzw = float4(r2.xyz + r9.xyz * r1.www, 1);\nreturn;\n}");
        for source in [
            &fixture,
            &fixture.replace("r9.", "r19.").replace(",r9,", ",r19,"),
        ] {
            let (adapted, changed) = adapt(source).unwrap();
            assert!(changed);
            assert_eq!(inputs::cb_count(&adapted, 3).unwrap(), None);
            assert_eq!(inputs::cb_count(&adapted, 8).unwrap(), Some(8));
            assert!(!adapted.contains("Texture3D"));
            assert!(adapted.contains("r2.xyz = r4.xzw;"));
            #[cfg(windows)]
            crate::d2_mot::native::shader::compile(&adapted, false).unwrap();
        }
        assert!(
            adapt(&fixture.replace("dot(r11.xyzw, r0.xyzw)", "dot(r12.xyzw, r0.xyzw)")).is_err()
        );
        let mut no_probes = fixture.lines().map(str::to_owned).collect::<Vec<_>>();
        probes(&mut no_probes).unwrap();
        let no_probes = no_probes.join("\n")
            .replace(&inputs::cb_decl(8, 36), &inputs::cb_decl(8, 8))
            .replace("TextureCube<float4> t24 : register(t24);", "")
            .replace("TextureCube<float4> t25 : register(t25);", "")
            .replace("r0.xyz = float3(0,0,1);", "r0.xyz = float3(0,0,1);\nr7.xyz = float3(0,0,-1);")
            .replace("r0.w = t19.", "r7.w = 1;\nr6.x = dot(r10.xyzw, r7.xyzw);\nr6.y = dot(r11.xyzw, r7.xyzw);\nr6.z = dot(r12.xyzw, r7.xyzw);\nr6.xyz = max(float3(0,0,0), r6.xyz);\nr0.w = t19.")
            .replace("r2.xyz + r9.xyz", "r2.xyz + r6.xyz + r9.xyz");
        let (adapted, changed) = adapt(&no_probes).unwrap();
        assert!(changed);
        assert!(adapted.contains("r6.y = dot(r11.xyzw, r7.xyzw);"));
        assert!(!adapted.contains("Texture3D"));
        #[cfg(windows)]
        crate::d2_mot::native::shader::compile(&adapted, false).unwrap();
    }
}
