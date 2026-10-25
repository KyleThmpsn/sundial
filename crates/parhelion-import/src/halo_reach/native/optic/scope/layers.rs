use super::*;
use crate::halo_reach::hud::scope::Scope;
use std::fmt::Write;

pub(super) fn pixel(scope: &Scope, slots: &[usize]) -> Result<String> {
    let extent = scope
        .layers
        .iter()
        .filter(|layer| layer.flags & 8 != 0)
        .map(|layer| layer.size[1] * layer.scale[1].abs())
        .fold(0f32, f32::max);
    ensure!(
        extent.is_finite() && extent > 0.,
        "Scope has no mask extent"
    );
    ensure!(
        slots.len() == scope.layers.len(),
        "Scope texture bindings differ"
    );
    let mut shader = String::from(
        "cbuffer Sight:register(b0) {float4 sight[19];};\n\
         cbuffer View:register(b12) {float4 view[13];};\n\
         SamplerState ScopeSampler:register(s0);\n",
    );
    for slot in slots
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        writeln!(shader, "Texture2D<float4> Map{slot}:register(t{slot});")?;
    }
    writeln!(
        shader,
        "void main(float2 uv:TEXCOORD0,out float4 color:SV_TARGET0) {{\n\
         float visibility=saturate(sight[3].w);clip(visibility-0.0001);\n\
         float aspect=clamp(view[12].w/max(view[12].z,0.000001),0.1,10);\n\
         float2 p=uv*float2(aspect,-1)*{:.9};color=0;",
        extent / 0.94
    )?;
    for (layer, slot) in scope.layers.iter().zip(slots) {
        writeln!(
            shader,
            "{{float2 q=(p-float2({:.9},{:.9}))/float2({:.9},{:.9});",
            layer.offset[0], layer.offset[1], layer.scale[0], layer.scale[1]
        )?;
        for (axis, flag) in [(0, 1), (1, 2)] {
            if layer.flags & flag != 0 {
                let lane = ["x", "y"][axis];
                writeln!(
                    shader,
                    "q.{lane}=abs(q.{lane})*{};",
                    if layer.origin[axis] <= 0.5 { 1 } else { -1 }
                )?;
            }
        }
        writeln!(
            shader,
            "q=q/float2({:.9},{:.9})+float2({:.9},{:.9});",
            layer.size[0], layer.size[1], layer.origin[0], layer.origin[1]
        )?;
        let coverage = if layer.flags & 8 != 0 {
            "1.0"
        } else {
            "step(0,q.x)*step(q.x,1)*step(0,q.y)*step(q.y,1)"
        };
        writeln!(
            shader,
            "float coverage={coverage};\n\
            float2 t=lerp(float2({:.9},{:.9}),float2({:.9},{:.9}),saturate(q));\n\
            float4 bitmap=Map{slot}.SampleLevel(ScopeSampler,t,0);\n\
            float alpha=saturate(bitmap.a*coverage*{:.9});",
            layer.uv[0], layer.uv[2], layer.uv[1], layer.uv[3], layer.opacity
        )?;
        let rgb = |v: [f32; 3]| format!("float3({:.9},{:.9},{:.9})", v[0], v[1], v[2]);
        let tint = if layer.shader == 6 {
            rgb(layer.color[0])
        } else {
            format!(
                "{}*bitmap.g+{}*(1-bitmap.g)+{}*bitmap.b",
                rgb(layer.color[0]),
                rgb(layer.color[1]),
                rgb(layer.color[2])
            )
        };
        writeln!(
            shader,
            "float3 tint={tint};color=float4(tint*alpha,alpha)+color*(1-alpha);}}"
        )?;
    }
    shader.push_str("color*=visibility;\n}\n");
    Ok(shader)
}
