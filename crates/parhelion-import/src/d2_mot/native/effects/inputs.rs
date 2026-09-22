//! Checked resource and constant-buffer rewrites for source pixel equations.
use super::*;
use crate::d2_mot::native::shader::replace_once;
use std::collections::BTreeSet;

pub(super) fn slots(text: &str, kind: char) -> Result<BTreeSet<u32>> {
    let marker = format!("register({kind}");
    text.match_indices(&marker)
        .map(|(at, _)| {
            Ok(text[at + marker.len()..]
                .split_once(')')
                .context("shader register")?
                .0
                .parse()?)
        })
        .collect()
}

pub(super) fn cb_count(text: &str, slot: usize) -> Result<Option<usize>> {
    let prefix = format!("cbuffer cb{slot} : register(b{slot})\n{{\n  float4 cb{slot}[");
    if let Some((_, tail)) = text.split_once(&prefix) {
        return Ok(Some(
            tail.split_once(']')
                .context("constant buffer length")?
                .0
                .parse()?,
        ));
    }
    ensure!(
        !text.contains(&format!("cbuffer cb{slot} ")),
        "uninspected constant buffer declaration"
    );
    Ok(None)
}

pub(super) fn cb_decl(slot: usize, count: usize) -> String {
    format!("cbuffer cb{slot} : register(b{slot})\n{{\n  float4 cb{slot}[{count}];\n}}")
}

fn pixel_view_programs(modern: &Payload, native: &Payload) -> Result<()> {
    let source = array_bytes(modern, 0x60, 1)?;
    let target = array_bytes(native, 0x58, 1)?;
    // Both programs write extern View + 0x10 to pixel cb12[13]. The modern
    // scope appends camera position at [14], while native shaders read it from
    // the camera-to-world matrix's translation vector at [7].
    ensure!(
        source.ends_with(&[0x4B, 2, 1, 0x52, 13, 0x4B, 2, 2, 0x52, 14])
            && target.ends_with(&[0x3D, 2, 1, 0x43, 13])
            && modern.array(0x90, 16, Some(0x80800090))?.len() == 15
            && native.array(0x88, 16, Some(0x80800090))?.len() == 14,
        "pixel view scope producers differ from the supported cross-build layout"
    );
    Ok(())
}

pub(super) fn validate_pixel_view(root: &Path) -> Result<()> {
    let mut scopes = Vec::new();
    for era in ["modern", "native"] {
        let folder = root.join(format!("tfx-{era}"));
        let context = load(&folder.join("context.json"))?;
        let views = context["scopes"]
            .as_array()
            .context("render scopes")?
            .iter()
            .filter(|s| s["name"] == "view")
            .collect::<Vec<_>>();
        ensure!(views.len() == 1, "missing or ambiguous view scope");
        scopes.push(Payload(fs::read(folder.join(format!(
            "raw/{}.bin",
            views[0]["tag"].as_str().context("view scope tag")?
        )))?));
    }
    pixel_view_programs(&scopes[0], &scopes[1]).map_err(crate::d2_mot::source_limit)
}

/// Resolve split and combined dye banks from the source material's enabled scopes.
pub(super) fn dye_inputs(
    text: &str,
    material: &Payload,
    root: &Path,
) -> Result<BTreeMap<usize, Option<usize>>> {
    let context = load(&root.join("tfx-modern/context.json"))?;
    let mut inputs = BTreeMap::new();
    for scope in context["scopes"].as_array().context("source scopes")? {
        let bank = match scope["name"].as_str() {
            Some("gear_dye_0") => Some(0),
            Some("gear_dye_1") => Some(1),
            Some("gear_dye_2") => Some(2),
            Some("gear_dye_012") => None,
            _ => continue,
        };
        let index = scope["index"].as_u64().context("source dye scope index")?;
        ensure!(index < 64, "source dye scope exceeds material mask");
        if material.u64(0x20)? & (1 << index) == 0 {
            continue;
        }
        let p = Payload(fs::read(root.join(format!(
            "tfx-modern/raw/{}.bin",
            scope["tag"].as_str().context("scope tag")?
        )))?);
        let slot = p.u32(0xB0)? as usize;
        if let Some(count) = cb_count(text, slot)? {
            let capacity = if bank.is_some() { 21 } else { 63 };
            ensure!(
                p.array(0x90, 16, Some(0x80800090))?.len() == capacity
                    && (1..=capacity).contains(&count),
                "source dye scope layout differs"
            );
            ensure!(
                inputs.insert(slot, bank).is_none(),
                "ambiguous source dye binding"
            );
        }
    }
    Ok(inputs)
}

pub(super) fn merge_dyes(
    text: &str,
    capacity: usize,
    inputs: &BTreeMap<usize, Option<usize>>,
) -> Result<String> {
    if inputs.is_empty() {
        return Ok(text.to_owned());
    }
    ensure!(capacity == 63, "source dye capacity differs");
    let count = cb_count(text, 0)?.unwrap_or(0);
    let mut text = text.to_owned();
    for (&slot, bank) in inputs {
        let declared = cb_count(&text, slot)?.context("missing source dye declaration")?;
        ensure!(
            (1..=if bank.is_some() { 21 } else { capacity }).contains(&declared),
            "source dye buffer differs"
        );
        text = replace_once(&text, &cb_decl(slot, declared), "")?;
        if let Some(bank) = bank {
            ensure!(*bank < 3, "source dye bank index");
            let indices =
                reads(&text, slot).context("dynamic split dye addressing is unsupported")?;
            for index in indices {
                ensure!(
                    index < declared,
                    "source split dye read outside declaration"
                );
                let mapped = if index < 3 {
                    bank * 3 + index
                } else {
                    9 + bank * 18 + index - 3
                };
                text = text.replace(
                    &format!("cb{slot}[{index}]"),
                    &format!("cb0[{}]", count + mapped),
                );
            }
            ensure!(
                !text.contains(&format!("cb{slot}[")),
                "unmapped source split dye read"
            );
        } else {
            text = text.replace(&format!("cb{slot}["), &format!("cb0[{count} + "));
        }
    }
    if count > 0 {
        text = replace_once(&text, &cb_decl(0, count), &cb_decl(0, count + capacity))?;
    } else {
        text = format!("{}\n{text}", cb_decl(0, capacity));
    }
    Ok(text)
}

pub(super) fn model_constants(text: &str, scale: f32) -> Result<String> {
    if let Some(count) = cb_count(text, 1)? {
        ensure!(
            count == 6 && scale.is_finite() && scale > 0.,
            "source model constant contract differs"
        );
        let mut text = replace_once(text, &cb_decl(1, count), "")?;
        for suffix in ["wwww", "www", "ww", "w"] {
            let value = if suffix.len() == 1 {
                format!("({scale:.9})")
            } else {
                format!(
                    "float{}({})",
                    suffix.len(),
                    vec![format!("{scale:.9}"); suffix.len()].join(",")
                )
            };
            text = text.replace(&format!("cb1[5].{suffix}"), &value);
        }
        ensure!(!text.contains("cb1["), "unmapped source model constant");
        Ok(text)
    } else {
        Ok(text.to_owned())
    }
}

pub(super) fn pixel(text: &str, rect: [usize; 4], size: [usize; 2]) -> Result<String> {
    let mut text = text.to_owned();
    if cb_count(&text, 12)? == Some(15) {
        text = replace_once(&text, &cb_decl(12, 15), &cb_decl(12, 14))?;
        text = text.replace("cb12[14].xyz", "cb12[7].xyz");
        if !reads(&text, 12).is_some_and(|indices| indices.iter().all(|i| *i < 14)) {
            return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
                "source pixel camera scope has an unmapped input beyond the native view layout"
            )));
        }
    }
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    let mut helpers = String::new();
    for slot in slots(&text, 't')?.into_iter().filter(|v| *v < 3) {
        if text.contains(&format!("t{slot}.Sample(")) {
            text = text.replace(&format!("t{slot}.Sample("), &format!("source_plate{slot}("));
            helpers.push_str(&format!("float4 source_plate{slot}(SamplerState s, float2 uv) {{ float2 scale = float2({w}.0/{aw}.0,{h}.0/{ah}.0); return t{slot}.SampleGrad(s, (saturate(uv) * float2({w},{h}) + float2({x},{y})) / float2({aw},{ah}), ddx(uv)*scale, ddy(uv)*scale); }}\n"));
        }
        ensure!(
            !text.contains(&format!("t{slot}.Load("))
                && !text.contains(&format!("t{slot}.SampleLevel("))
                && !text.contains(&format!("t{slot}.SampleGrad(")),
            "source plate sampling requires another addressing contract"
        );
    }
    replace_once(&text, "void main(\n", &format!("{helpers}\nvoid main(\n"))
}

pub(super) fn reads(text: &str, slot: usize) -> Option<BTreeSet<usize>> {
    let body = text.split_once("void main(")?.1;
    let prefix = format!("cb{slot}[");
    body.match_indices(&prefix)
        .map(|(at, _)| {
            body[at + prefix.len()..]
                .split_once(']')?
                .0
                .trim()
                .parse()
                .ok()
        })
        .collect()
}

pub(super) fn pixel_scopes(text: &str, stage: usize) -> Result<()> {
    let allowed = if stage == 0 {
        BTreeSet::from([0, 12])
    } else {
        BTreeSet::from([0, 8, 12, 13])
    };
    let unsupported = slots(text, 'b')?
        .difference(&allowed)
        .copied()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        // These bindings belong to the source shader. Another animation or
        // geometry donor cannot supply a missing renderer-scope conversion.
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source pixel stage {stage} has unmapped constant buffers {unsupported:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_dyes_preserve_bank_order_and_material_constants() {
        for bank in 0..3 {
            let source = format!(
                "{}\n{}\nvoid main(\nout float4 color : SV_TARGET) {{ color = cb0[0] + cb5[0] + cb5[2] + cb5[3] + cb5[20]; }}",
                cb_decl(0, 1),
                cb_decl(5, 21)
            );
            let mapped = merge_dyes(&source, 63, &BTreeMap::from([(5, Some(bank))])).unwrap();
            assert_eq!(
                reads(&mapped, 0).unwrap(),
                BTreeSet::from([
                    0,
                    1 + bank * 3,
                    3 + bank * 3,
                    10 + bank * 18,
                    27 + bank * 18
                ])
            );
            assert_eq!(cb_count(&mapped, 0).unwrap(), Some(64));
            assert_eq!(cb_count(&mapped, 5).unwrap(), None);
            #[cfg(windows)]
            crate::d2_mot::native::shader::compile(&mapped, false).unwrap();
            assert!(
                merge_dyes(
                    &source.replace("cb5[3]", "cb5[(int)cb0[0].x]"),
                    63,
                    &BTreeMap::from([(5, Some(bank))])
                )
                .is_err()
            );
        }
    }

    #[test]
    fn unmapped_pixel_scopes_stop_donor_retries() {
        let source = cb_decl(5, 4);
        let error = pixel_scopes(&source, 7).unwrap_err();
        assert!(crate::d2_mot::is_source_limit(&error));
        assert!(format!("{error:#}").contains("[5]"));
        pixel_scopes(&cb_decl(12, 14), 0).unwrap();
        pixel_scopes(&cb_decl(13, 2), 7).unwrap();
        assert!(pixel_scopes(&cb_decl(13, 2), 0).is_err());
    }

    fn view_scope(code_at: usize, values_at: usize, count: usize, code: &[u8]) -> Payload {
        let mut bytes = vec![0; 0x500];
        for (at, header, n, class) in [
            (code_at, 0x200, code.len(), 0x80800009u32),
            (values_at, 0x300, count, 0x80800090),
        ] {
            bytes[at..at + 8].copy_from_slice(&(n as u64).to_le_bytes());
            bytes[at + 8..at + 16]
                .copy_from_slice(&(header as i64 - (at + 8) as i64).to_le_bytes());
            bytes[header..header + 8].copy_from_slice(&(n as u64).to_le_bytes());
            bytes[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
        }
        bytes[0x210..0x210 + code.len()].copy_from_slice(code);
        Payload(bytes)
    }

    #[test]
    fn dye_binding_uses_enabled_scope_metadata_and_rejects_ambiguity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let scope_dir = root.join("tfx-modern");
        fs::create_dir_all(scope_dir.join("raw")).unwrap();
        let mut scope = view_scope(0x60, 0x90, 21, &[]);
        put(&mut scope.0, 0xB0, &5u32.to_le_bytes()).unwrap();
        fs::write(scope_dir.join("raw/fixture.bin"), &scope.0).unwrap();
        let mut context = json!({"scopes":[{"name":"gear_dye_2","index":28,"tag":"fixture"}]});
        write_json(&scope_dir.join("context.json"), &context).unwrap();
        let mut material = Payload(vec![0; 0x40]);
        let shader = cb_decl(5, 4);
        assert!(dye_inputs(&shader, &material, root).unwrap().is_empty());
        put(&mut material.0, 0x20, &(1u64 << 28).to_le_bytes()).unwrap();
        assert_eq!(
            dye_inputs(&shader, &material, root).unwrap(),
            BTreeMap::from([(5, Some(2))])
        );
        context["scopes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"gear_dye_1","index":29,"tag":"fixture"}));
        write_json(&scope_dir.join("context.json"), &context).unwrap();
        put(
            &mut material.0,
            0x20,
            &((1u64 << 28) | (1u64 << 29)).to_le_bytes(),
        )
        .unwrap();
        assert!(dye_inputs(&shader, &material, root).is_err());
    }

    #[test]
    fn view_mapping_requires_identical_extern_producers_and_real_capacity() {
        let modern = view_scope(
            0x60,
            0x90,
            15,
            &[0x4B, 2, 1, 0x52, 13, 0x4B, 2, 2, 0x52, 14],
        );
        let native = view_scope(0x58, 0x88, 14, &[0x3D, 2, 1, 0x43, 13]);
        pixel_view_programs(&modern, &native).unwrap();
        assert!(
            pixel_view_programs(
                &modern,
                &view_scope(0x58, 0x88, 13, &[0x3D, 2, 1, 0x43, 13])
            )
            .is_err()
        );
        assert!(
            pixel_view_programs(
                &modern,
                &view_scope(0x58, 0x88, 14, &[0x3D, 2, 2, 0x43, 13])
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "Requires explicitly configured exported source shader and render inputs"]
    fn configured_pixel_camera_mapping_compiles() {
        let root = std::env::var_os("PARHELION_IMPORT_RENDER_INPUTS")
            .expect("PARHELION_IMPORT_RENDER_INPUTS");
        let source = std::env::var_os("PARHELION_IMPORT_PIXEL_SHADER")
            .expect("PARHELION_IMPORT_PIXEL_SHADER");
        validate_pixel_view(Path::new(&root)).unwrap();
        let source = fs::read_to_string(source).unwrap().replace("\r\n", "\n");
        let mapped = pixel(&source, [0, 0, 4, 4], [4, 4]).unwrap();
        assert_eq!(cb_count(&mapped, 12).unwrap(), Some(14));
        #[cfg(windows)]
        crate::d2_mot::native::shader::compile(&mapped, false).unwrap();
    }

    #[test]
    fn camera_inputs_are_remapped_or_rejected_before_shader_compilation() {
        let source = format!(
            "{}\nvoid main(\nout float4 color : SV_TARGET) {{ color = float4(cb12[14].xyz, 1); }}",
            cb_decl(12, 15)
        );
        let mapped = pixel(&source, [0, 0, 4, 4], [4, 4]).unwrap();
        assert!(mapped.contains("cb12[7].xyz"));
        let retained = pixel(
            &source.replace("cb12[14].xyz", "cb12[13].xyz"),
            [0, 0, 4, 4],
            [4, 4],
        )
        .unwrap();
        assert_eq!(cb_count(&retained, 12).unwrap(), Some(14));
        assert!(retained.contains("cb12[13].xyz"));
        #[cfg(windows)]
        crate::d2_mot::native::shader::compile(&retained, false).unwrap();
        let error = pixel(
            &source.replace("cb12[14].xyz", "cb12[15].xyz"),
            [0, 0, 4, 4],
            [4, 4],
        )
        .unwrap_err();
        assert!(crate::d2_mot::is_source_limit(&error));
    }

    #[test]
    fn partial_dye_declarations_keep_full_runtime_storage_and_material_constants() {
        let source = format!(
            "{}\n{}\nvoid main(\nout float4 output : SV_TARGET0) {{ output = cb0[12] + cb7[6]; }}",
            cb_decl(0, 13),
            cb_decl(7, 7)
        );
        let result = merge_dyes(&source, 63, &BTreeMap::from([(7, None)])).unwrap();
        assert_eq!(cb_count(&result, 0).unwrap(), Some(76));
        assert_eq!(reads(&result, 7), Some(BTreeSet::new()));
        assert!(result.contains("output = cb0[12] + cb0[13 + 6]"));
        assert!(merge_dyes(&source, 6, &BTreeMap::from([(7, None)])).is_err());
        #[cfg(windows)]
        crate::d2_mot::native::shader::compile(&result, false).unwrap();
    }
}
