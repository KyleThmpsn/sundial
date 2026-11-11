use super::*;
use crate::halo_reach::hud::scope::Scope;
use std::fmt::Write;

/// The overlay's pixel program. `sampler` is the register the material's expression binds the
/// reticle template's linear clamp sampler to.
pub(super) fn pixel(scope: &Scope, slots: &[usize], sampler: u8) -> Result<String> {
    ensure!(
        slots.len() == scope.layers.len(),
        "Scope texture bindings differ"
    );
    let hud = &scope.frame;
    let mut shader = format!(
        "cbuffer Sight:register(b0) {{float4 sight[19];}};\n\
         cbuffer View:register(b12) {{float4 view[13];}};\n\
         cbuffer Frame:register(b13) {{float4 frame[2];}};\n\
         SamplerState ScopeSampler:register(s{sampler});\n",
    );
    // The source curve through its nine points, indexed by column and row of the virtual grid.
    let points = hud
        .curvature
        .iter()
        .map(|p| format!("float2({:.9},{:.9})", p[0], p[1]))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(
        shader,
        "static const float2 curvature[9]={{{points}}};\n\
         float2 curve(float2 h){{\n\
         float3 a=float3(h.x*(h.x-1)/2,1-h.x*h.x,h.x*(h.x+1)/2);\n\
         float3 b=float3(h.y*(h.y-1)/2,1-h.y*h.y,h.y*(h.y+1)/2);\n\
         float2 r=0;[unroll]for(int i=0;i<3;i++){{[unroll]for(int j=0;j<3;j++){{r+=a[i]*b[j]*curvature[i*3+j];}}}}\n\
         return r;}}"
    )?;
    // The top level's texels from `lo` to `hi`, filtered between the four nearest and held at the
    // rectangle's edge texels. It fetches without a sampler, so no address mode, level selection
    // or texel outside the sprite can reach an extended border. The rectangle is given in texels
    // of a texture `size` wide and scales to the level the view exposes, which is smaller while a
    // streamed texture's top levels are still loading.
    shader.push_str(
        "float4 edge(Texture2D<float4> map,float2 lo,float2 hi,float2 size,float2 q){\n\
         float width,height,levels;map.GetDimensions(0,width,height,levels);\n\
         float2 s=float2(width,height)/size;lo*=s;hi=max(hi*s,lo+1);\n\
         float2 f=clamp(lerp(lo,hi,saturate(q))-0.5,lo,hi-1);\n\
         int2 a=int2(floor(f));float2 k=f-a;int2 b=min(a+1,int2(hi-1));\n\
         return lerp(lerp(map.Load(int3(a,0)),map.Load(int3(b.x,a.y,0)),k.x),\
         lerp(map.Load(int3(a.x,b.y,0)),map.Load(int3(b,0)),k.x),k.y);}\n",
    );
    for slot in slots
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        writeln!(shader, "Texture2D<float4> Map{slot}:register(t{slot});")?;
    }
    // The slope of the curve at the centre steps the inversion. The curve differs from that line by
    // a few percent inside the canvas, so a few steps converge.
    let slope = [
        (hud.curvature[7][0] - hud.curvature[1][0]) / 2.,
        (hud.curvature[5][1] - hud.curvature[3][1]) / 2.,
    ];
    writeln!(
        shader,
        // Row three holds aim visibility in its first lane, as native reticles read it. The
        // overlay appears past 0.99, where the weapon's own surfaces discard. HUD units run from
        // the screen centre, the source crosshair anchor, with Y down. The virtual canvas keeps
        // its own aspect at the screen's height, so wider screens extend past its sides.
        "void main(float2 uv:TEXCOORD0,out float4 color:SV_TARGET0) {{\n\
         float visibility=saturate((sight[3].x-0.99)*100);clip(visibility-0.0001);\n\
         float aspect=clamp(view[12].w/max(view[12].z,0.000001),0.1,10);\n\
         float2 c=uv*float2(aspect*{:.9},-1)/float2({:.9},{:.9});\n\
         float2 h=c/float2({:.9},{:.9});\n\
         [unroll]for(int k=0;k<6;k++){{h+=(c-curve(h))/float2({:.9},{:.9});}}\n\
         float2 p=h*float2({:.9},{:.9});color=0;",
        hud.size[1] / hud.size[0],
        hud.safe[0],
        hud.safe[1],
        slope[0],
        slope[1],
        slope[0],
        slope[1],
        hud.size[0] / 2.,
        hud.size[1] / 2.
    )?;
    for (layer, slot) in scope.layers.iter().zip(slots) {
        writeln!(
            shader,
            "{{float2 q=(p-float2({:.9},{:.9}))/float2({:.9},{:.9});",
            layer.offset[0], layer.offset[1], layer.scale[0], layer.scale[1]
        )?;
        // A rotated widget turns about its registration point. Screen Y runs down, so the
        // source's positive angle turns the sprite's leading edge from +X toward +Y.
        if layer.rotation != 0. {
            let (sin, cos) = layer.rotation.sin_cos();
            writeln!(
                shader,
                "q=float2(q.x*{cos:.9}+q.y*{sin:.9},q.y*{cos:.9}-q.x*{sin:.9});"
            )?;
        }
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
        // Animation scale stretches the sprite about its registration point before rotation, as
        // the needle rifle's corner ticks narrow and then turn 45 degrees.
        writeln!(
            shader,
            "q=q/float2({:.9},{:.9})+float2({:.9},{:.9});",
            layer.size[0] * layer.stretch[0],
            layer.size[1] * layer.stretch[1],
            layer.origin[0],
            layer.origin[1]
        )?;
        // An extended border repeats the bitmap's edge texels across the screen. It reads the top
        // level through `edge`, held at the sprite's own edge texels. Sampled at its edge instead,
        // the sniper rifle's 394 by 250 mask drew past its extent as a faint veil crossed by
        // lines in gameplay, while the same bytes drew the full border under WARP.
        // Other sprites take the texture's mips through the screen-space footprint, as Reach's
        // HUD shaders filter them. MCC's triple-sized reticle art covers three texels per pixel
        // at full HD, and its 9-texel tick lines alias when sampled at the top level alone.
        if layer.flags & 8 != 0 {
            // The texture's size in texels, from the sprite's texels and its nonempty share of it.
            let size = [
                layer.texels[0] / (layer.uv[1] - layer.uv[0]),
                layer.texels[1] / (layer.uv[3] - layer.uv[2]),
            ];
            writeln!(
                shader,
                "float coverage=1.0;\n\
                float4 bitmap=edge(Map{slot},float2({:.9},{:.9}),float2({:.9},{:.9}),float2({:.9},{:.9}),q);\n\
                float alpha=saturate(bitmap.a*coverage*{:.9});",
                layer.uv[0] * size[0],
                layer.uv[2] * size[1],
                layer.uv[1] * size[0],
                layer.uv[3] * size[1],
                size[0],
                size[1],
                layer.opacity
            )?;
        } else {
            writeln!(
                shader,
                "float coverage=step(0,q.x)*step(q.x,1)*step(0,q.y)*step(q.y,1);\n\
                float2 t=lerp(float2({:.9},{:.9}),float2({:.9},{:.9}),saturate(q));\n\
                float4 bitmap=Map{slot}.Sample(ScopeSampler,t);\n\
                float alpha=saturate(bitmap.a*coverage*{:.9});",
                layer.uv[0], layer.uv[2], layer.uv[1], layer.uv[3], layer.opacity
            )?;
        }
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
    // Reach blends its HUD over the displayed image, while the reticle pass blends into the linear
    // scene before tone mapping. Raising the scene's remainder to the display gamma keeps the
    // source's dimming: a border at 0.66 coverage leaves about a third of the displayed scene,
    // near the quarter to third a capture of MCC's zoomed Magnum leaves, rather than about two
    // thirds. Layer colors are unchanged.
    // Source HUD colors are linear. Native reticles undo the scene exposure through frame row one
    // and then apply the display mapping in rows 6 to 11, so the overlay keeps its source color
    // under any exposure and on HDR displays. The mapping follows native reticle program
    // `81532F5D`.
    shader.push_str(
        "color.a=1-pow(saturate(1-color.a),2.2);\n\
         color*=visibility;color.rgb*=frame[1].z*frame[1].w;\n\
         float4 hdr=float4(pow(max(color.rgb,0),sight[8].x)*sight[6].x,color.a);\n\
         hdr.rgb=max(hdr.rgb*(10000/sight[10].x)+sight[11].x,0);\n\
         if(sight[9].x>0){\n\
         float3 n=mul(float3x3(0.6274,0.3293,0.0433,0.0691,0.9195,0.0114,0.0164,0.088,0.8956),hdr.rgb)*sight[9].x*0.0001;\n\
         float3 m=pow(n,0.159302);\n\
         hdr.rgb=pow((m*18.851562+0.835938)/(m*18.6875+1),78.84375);}\n\
         if(sight[6].x>0){color=hdr;}\n\
         if(sight[7].x>0){color=float4(pow(max(color.rgb/sight[7].x,0),1/sight[8].x),color.a);}\n\
         }\n",
    );
    Ok(shader)
}
