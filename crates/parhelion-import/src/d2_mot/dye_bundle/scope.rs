//! Translate the source shader's complete numeric material program, including live clocks.
//! Unmapped source behavior fails the import instead of retaining a donor's animation.
use super::*;
use crate::d2_mot::tfx::program::{self, Bindings};
use std::collections::BTreeMap;
mod emission;
pub(super) mod layout;

fn array(p: &Payload, at: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.array(at, stride, Some(class))?
        .iter()
        .flat_map(|&row| p.0[row..row + stride].iter().copied())
        .collect())
}

pub(super) fn append(
    data: &mut Vec<u8>,
    at: usize,
    rows: &[u8],
    stride: usize,
    class: u32,
) -> Result<()> {
    ensure!(
        rows.len().is_multiple_of(stride),
        "shader scope array stride differs"
    );
    if rows.is_empty() {
        data[at..at + 16].fill(0);
        return Ok(());
    }
    let count = rows.len() / stride;
    let header = (data.len() + 19) & !15;
    data.resize(header - 4, 0);
    data.extend(0x80809FBDu32.to_le_bytes());
    data.extend((count as u64).to_le_bytes());
    data.extend((class as u64).to_le_bytes());
    data.extend(rows);
    data[at..at + 8].copy_from_slice(&(count as u64).to_le_bytes());
    data[at + 8..at + 16].copy_from_slice(&(header as i64 - (at + 8) as i64).to_le_bytes());
    let length = data.len() as u64;
    data[..8].copy_from_slice(&length.to_le_bytes());
    Ok(())
}

fn globals(source: &Path, native: &Path) -> Result<BTreeMap<u8, u8>> {
    let read = |path: &Path| -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            path.join("render-context.json"),
        )?)?)
    };
    let (modern, old) = (read(source)?, read(native)?);
    let mut mapped = BTreeMap::new();
    for m in modern["channels"]
        .as_array()
        .context("source render channels")?
    {
        if let Some(n) = old["channels"]
            .as_array()
            .context("native render channels")?
            .iter()
            .find(|n| n["hash"] == m["hash"])
        {
            mapped.insert(
                u8::try_from(m["index"].as_u64().context("source channel")?)?,
                u8::try_from(n["index"].as_u64().context("native channel")?)?,
            );
        }
    }
    Ok(mapped)
}

// Shared renderer lookups can change format, dimensions, mip layout or palette contents.
// A matching row index is insufficient proof, especially when a live program changes it.
// Require identical lookup payloads rather than silently changing iridescent/specular effects.
pub(super) fn validate_lookups(source: &Path, native: &Path) -> Result<Value> {
    let read = |path: &Path| -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            path.join("render-context.json"),
        )?)?)
    };
    let (modern, old) = (read(source)?, read(native)?);
    let mut evidence = vec![];
    for name in [
        "specular_tint",
        "specular_lobe",
        "specular_lobe_3d",
        "iridescence",
    ] {
        let texture = |context: &Value| -> Result<Value> {
            Ok(context["lookup_textures"]
                .as_array()
                .context("render lookup textures")?
                .iter()
                .find(|t| t["name"] == name)
                .with_context(|| format!("missing {name} render lookup"))?
                .clone())
        };
        let (m, n) = (texture(&modern)?, texture(&old)?);
        let mh = Payload(hex::decode(
            m["header"].as_str().context("source lookup header")?,
        )?);
        let nh = Payload(hex::decode(
            n["header"].as_str().context("native lookup header")?,
        )?);
        ensure!(
            mh.u32(4)? == nh.u32(4)?
                && mh.0[34..42] == nh.0[14..22]
                && mh.0[44..46] == nh.0[22..24],
            "source shader uses incompatible {name} lookup layout"
        );
        let mut data = vec![];
        let large = mh.u32(60)?;
        if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
            data.extend(fs::read(
                source.join("raw").join(format!("{large:08X}.bin")),
            )?);
        }
        data.extend(fs::read(source.join("raw").join(format!(
            "{}.bin",
            m["buffer"].as_str().context("source lookup buffer")?
        )))?);
        let native_data = fs::read(native.join("raw").join(format!(
            "{}.bin",
            n["buffer"].as_str().context("native lookup buffer")?
        )))?;
        ensure!(
            data.len() == mh.u32(0)? as usize
                && native_data.len() == nh.u32(0)? as usize
                && data == native_data,
            "source shader uses a different {name} lookup requiring renderer conversion"
        );
        evidence.push(json!({"name":name,"identical":true,"bytes":data.len()}));
    }
    Ok(json!(evidence))
}

pub(super) fn convert(
    source: &Path,
    native: &Path,
    material: &Value,
    target: &mut Vec<u8>,
) -> Result<Value> {
    let source_scope = material["scope"].as_str().context("source scope")?;
    let scope = Payload(fs::read(
        source.join("raw").join(format!("{source_scope}.bin")),
    )?);
    ensure!(
        scope.0.len() >= 0x378 && target.len() >= 0x3D0,
        "shader scope layout differs"
    );
    // The inspected dye family binds only pixel resources. Any extra stage must be
    // translated explicitly before this source can be offered as a complete import.
    for stage in [0xD0, 0x158, 0x1E0, 0x268, 0x2F0] {
        for offset in [0, 0x18, 0x28, 0x38, 0x48] {
            ensure!(
                scope.u64(stage + offset)? == 0,
                "source shader has unsupported material stage at 0x{stage:X}"
            );
        }
        ensure!(
            [0, u32::MAX, 0x811C9DC5].contains(&scope.u32(stage + 0x6C)?),
            "source shader has an additional constant buffer at 0x{stage:X}"
        );
        ensure!(
            scope.0[stage + 0x70..stage + 0x88].iter().all(|b| *b == 0),
            "source shader has unsupported stage metadata at 0x{stage:X}"
        );
    }
    ensure!(
        scope.u64(0x80)? == 0,
        "source shader has custom samplers requiring conversion"
    );
    let code = array(&scope, 0x60, 1, 0x80800009)?;
    let constants = array(&scope, 0x70, 16, 0x80800090)?;
    let mut remapped = Vec::new();
    for i in program::parse(&code)? {
        ensure!(
            !matches!(i.op, 0x53 | 0x56..=0x5B | 0x60..=0x62),
            "source shader uses unsupported material resource operation 0x{:02X}",
            i.op
        );
        remapped.push(i.op);
        if matches!(i.op, 0x51 | 0x52) {
            let output = VECTORS
                .iter()
                .find(|(from, _)| *from == usize::from(i.args[0]))
                .context("source shader output has no native material equivalent")?
                .1;
            remapped.push(u8::try_from(output)?);
        } else {
            remapped.extend(i.args);
        }
    }
    let lowered = program::lower(
        &remapped,
        &Bindings {
            globals: globals(source, native)?,
            constant_count: constants.len() / 16,
            output_count: 27,
            ..Default::default()
        },
    )?;
    lowered.require_runtime_inputs()?;
    // Remove every donor expression, expression constant and sampler. Preserve the source's
    // allocation flags and buffer slot while rebuilding its native field locations.
    append(target, 0x58, &lowered.code, 1, 0x80800009)?;
    append(target, 0x68, &constants, 16, 0x80800090)?;
    append(target, 0x78, &[], 16, 0x80807216)?;
    target[0x10..0x40].copy_from_slice(&scope.0[0x10..0x40]);
    target[0x50..0x58].copy_from_slice(&scope.0[0x58..0x60]);
    // Native dynamic constants have an extra 16-byte field before metadata.
    // The writable-output flag is at native +0xAC, not +0x9C. An external
    // dynamic buffer alone does not give the interpreter its output storage.
    target[0x98..0xA8].fill(0);
    target[0xA8..0xB8].copy_from_slice(&scope.0[0xA0..0xB0]);
    target[0xB8..0xBC].copy_from_slice(&scope.0[0xB0..0xB4]);
    ensure!(
        scope.0[0xB8..0xD0].iter().all(|b| *b == 0),
        "source shader has unsupported pixel stage metadata"
    );
    target[0xC0..0xD8].fill(0);
    for (from, to) in [
        (0xD0, 0xD8),
        (0x158, 0x170),
        (0x1E0, 0x208),
        (0x268, 0x2A0),
        (0x2F0, 0x338),
    ] {
        target[to..to + 0x98].fill(0);
        target[to + 0x10..to + 0x18].copy_from_slice(&scope.0[from + 0x10..from + 0x18]);
        target[to + 0x68..to + 0x78].copy_from_slice(&scope.0[from + 0x58..from + 0x68]);
        target[to + 0x78..to + 0x80].copy_from_slice(&scope.0[from + 0x68..from + 0x70]);
    }
    // At this point the vector mapping has revision 2 semantics. The final
    // normalization also moves emission-enable components into native selectors.
    layout::normalize_scope(target, Some(2))?;
    let converted = Payload(target.clone());
    let native_code = array(&converted, 0x58, 1, 0x80800009)?;
    let native_constants = array(&converted, 0x68, 16, 0x80800090)?;
    Ok(
        json!({"source_scope":source_scope,"source_program":hex::encode(code),"native_program":hex::encode(native_code),"expression_constants":hex::encode(native_constants),"outputs":lowered.evidence,"animation_preserved":true,"donor_program_retained":false,
            "native_program_before_components":hex::encode(&lowered.code),
            "source_expression_constants":hex::encode(constants),
            "emission_selectors_translated":true,
            "native_scope_layout":layout::REVISION,"source_output_flags":scope.u32(0xA4)?,
            "native_output_flags":u32::from_le_bytes(target[0xAC..0xB0].try_into()?),
            "gameplay_verified":false}),
    )
}
