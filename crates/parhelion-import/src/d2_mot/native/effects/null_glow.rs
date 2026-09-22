//! Specialize the inspected glow shader when its mask is explicitly unbound.
use super::*;
use crate::d2_mot::native::shader::replace_once;

const BLOCK: &str = "  r2.xyz = saturate(float3(4,4,4) * r1.xyz);\n  r3.xyz = saturate(float3(-0.25,-0.25,-0.25) + r1.xyz);\n  r2.xyz = cb7[3].xyz * r2.xyz + r3.xyz;\n  r0.w = t6.Sample(s3_s, v3.xy).x;\n  r2.xyz = r2.xyz + -r1.xyz;\n  r1.xyz = r0.www * r2.xyz + r1.xyz;\n";

pub(super) fn specialize(
    text: &str,
    material: &Payload,
    binding: &Value,
) -> Result<(String, bool)> {
    if material.u32(0x2B0)? != 0x80CF73DF {
        return Ok((text.to_owned(), false));
    }
    let rows = binding["textures"]
        .as_array()
        .context("glow texture bindings")?;
    let mask = rows.iter().filter(|v| v["slot"] == 6).collect::<Vec<_>>();
    ensure!(
        mask.len() == 1 && mask[0]["unbound"] == true,
        "glow mask is not explicitly unbound"
    );
    let code = array_bytes(material, 0x2D0, 1)?;
    ensure!(
        !program::parse(&code)?
            .iter()
            .any(|v| v.op == 0x56 && v.args == [0x26]),
        "glow mask has a runtime texture binding"
    );
    // D3D11 Functional Specification 7.18.17: Sampling Unbound Data.
    // https://microsoft.github.io/DirectX-Specs/d3d/archive/D3D11_3_FunctionalSpec.htm
    // D3D11 reads from an unbound SRV return zero. The inspected six-line
    // expression is lerp(r1, tint(r1), mask), so a null mask preserves r1.
    let text = replace_once(text, BLOCK, "")?;
    let text = replace_once(&text, &inputs::cb_decl(7, 4), "")?;
    let text = replace_once(&text, "Texture2D<float4> t6 : register(t6);", "")?;
    ensure!(
        !text.contains("cb7[") && !text.contains("t6."),
        "glow mask has other consumers"
    );
    Ok((text, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_verified_null_mask_removes_the_tint_branch() {
        let mut mat = Payload(vec![0; 0x400]);
        put(&mut mat.0, 0x2B0, &0x80CF73DFu32.to_le_bytes()).unwrap();
        let source = format!(
            "{}\nTexture2D<float4> t6 : register(t6);\n{BLOCK}  r1.xyz = cb0[13].xxx * r1.xyz;",
            inputs::cb_decl(7, 4)
        );
        let binding = json!({"textures":[{"slot":6,"unbound":true}]});
        let (result, changed) = specialize(&source, &mat, &binding).unwrap();
        assert!(changed && result.contains("r1.xyz = cb0[13]") && !result.contains("cb7"));
        assert!(
            specialize(
                &source,
                &mat,
                &json!({"textures":[{"slot":6,"tag":"12345678"}]})
            )
            .is_err()
        );
        assert!(
            specialize(
                &source.replace("r0.www * r2.xyz", "r0.www + r2.xyz"),
                &mat,
                &binding
            )
            .is_err()
        );
        append_array(&mut mat.0, 0x2D0, 0x80800009, &[0x4D, 3, 27, 0x56, 0x26], 1).unwrap();
        assert!(specialize(&source, &mat, &binding).is_err());
    }
}
