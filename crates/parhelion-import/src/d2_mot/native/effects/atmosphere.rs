//! Retain transmission when the preview renderer has no atmosphere resource.
use super::*;
use crate::d2_mot::native::shader::replace_once;

const HELPER: &str = "float4 source_atmosphere(SamplerState atmos_sampler, float2 uv, float mip) {\n  uint width, height;\n  t15.GetDimensions(width, height);\n  if (width == 0 || height == 0) return float4(1, 1, 1, 0);\n  return t15.SampleLevel(atmos_sampler, uv, mip);\n}\n";

pub(super) fn adapt(text: &str) -> Result<String> {
    if !inputs::slots(text, 't')?.contains(&15) {
        return Ok(text.to_owned());
    }
    ensure!(
        text.contains("Texture2D<float4> t15 : register(t15);")
            && text.matches("t15.").count() == 1
            && text.contains("t15.SampleLevel("),
        "uninspected atmospheric transmission resource"
    );
    // resinfo/GetDimensions on an unbound D3D11 resource returns zero.
    // https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/resinfo--sm4---asm-
    // A missing atmosphere transmits the surface unchanged. A bound lookup,
    // including a legitimately black fog sample, keeps its authored behavior.
    let text = text.replace("t15.SampleLevel(", "source_atmosphere(");
    replace_once(&text, "void main(\n", &format!("{HELPER}\nvoid main(\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_atmosphere_compiles_and_preserves_bound_sampling() {
        let source = "Texture2D<float4> t15 : register(t15);\nSamplerState s2_s : register(s2);\nvoid main(\nfloat4 position : SV_POSITION, out float4 color : SV_TARGET) { color = t15.SampleLevel(s2_s, position.xy, 0); }";
        let adapted = adapt(source).unwrap();
        #[cfg(windows)]
        crate::d2_mot::native::shader::compile(&adapted, false).unwrap();
        assert!(adapted.contains("return t15.SampleLevel(atmos_sampler, uv, mip);"));
        assert!(adapt(&source.replace("SampleLevel", "SampleGrad")).is_err());
        assert!(adapt(&source.replace("Texture2D", "Texture3D")).is_err());
        assert_eq!(adapt("void main() {}").unwrap(), "void main() {}");
    }
}
