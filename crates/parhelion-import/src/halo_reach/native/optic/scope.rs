//! A dedicated reticle pass follows the native sight's named visibility input.
use super::*;
use crate::presentation::append;

const VERTEX: &str = r#"
void main(float4 p:POSITION0,
out float2 uv:TEXCOORD0,out float4 position:SV_POSITION0) {
uv=p.xy;position=float4(p.xy,0.5,1);
}
"#;

const PIXEL: &str = r#"
cbuffer Sight:register(b0) { float4 sight[19]; };
cbuffer View:register(b12) { float4 view[13]; };
float stroke(float2 p,float2 a,float2 b,float width,float aa) {
float2 d=b-a;float t=saturate(dot(p-a,d)/dot(d,d));
return 1-smoothstep(width,width+aa,length(p-a-t*d));
}
void main(float2 uv:TEXCOORD0,out float4 color:SV_TARGET0) {
float visibility=saturate(sight[3].w);
clip(visibility-0.0001);
float aspect=clamp(view[12].w/max(view[12].z,0.000001),0.1,10);
float2 p=uv.xy*float2(aspect,1);
float2 aa=max(fwidth(p),float2(0.0001,0.0001));
float radius=0.88;
float distance=length(p);
float outside=smoothstep(radius-aa.y,radius+aa.y,distance);
#if RANGEFINDER
float ink=1-smoothstep(0.0017,0.0017+aa.y,abs(distance-(radius-0.026)));
float2 side=float2(abs(p.x),p.y);
ink=max(ink,stroke(side,float2(0.33,0),float2(0.83,0),0.001,aa.y));
for(int i=0;i<8;++i) {
float x=0.145+i*0.017;
ink=max(ink,stroke(side,float2(x,0),float2(x,-(i%2==0?0.028:0.018)),0.0008,aa.x));
}
for(int i=0;i<3;++i) {
float x=0.33+i*0.245;
ink=max(ink,stroke(side,float2(x,0),float2(x,-0.028),0.001,aa.x));
}
for(int i=0;i<4;++i) {
float y=-0.17-i*0.086;float width=0.018-i*0.003;
ink=max(ink,stroke(side,float2(0,y),float2(width,y-0.022),0.0015,aa.y));
}
ink=max(ink,stroke(p,float2(0,-0.61),float2(0,-0.852),0.003,aa.x));
ink=max(ink,stroke(p,float2(0,-0.46),float2(0,-0.61),0.0008,aa.x));
ink=max(ink,stroke(p,float2(-0.30,-0.18),float2(-0.30,-0.57),0.001,aa.x));
ink=max(ink,stroke(p,float2(-0.47,-0.16),float2(-0.40,-0.22),0.006,aa.y));
ink=max(ink,stroke(p,float2(-0.40,-0.22),float2(-0.37,-0.56),0.006,aa.x));
float angle=atan2(p.y,p.x);
float arc=(1-smoothstep(0.011,0.011+aa.y,abs(distance-0.76)))*step(0.33,angle)*step(angle,1.04);
ink=max(ink,arc);
for(int i=0;i<7;++i) {
float y=0.25+i*0.068;float x=sqrt(max(0,0.76*0.76-y*y));
ink=max(ink,stroke(p,float2(x-0.095,y),float2(x+0.027,y),i==6?0.004:0.001,aa.y));
}
float ring=1-smoothstep(0.003,0.003+aa.y,abs(distance-0.046));
float dot=1-smoothstep(0.004,0.004+aa.y,distance);
float mark=max(ring,dot);
float alpha=max(outside,max(ink,mark))*visibility;
float3 tint=lerp(float3(0.015,0.017,0.019),float3(0.38,0.72,0.92),mark);
color=float4(tint*alpha,alpha);
#else
float rim=1-smoothstep(0.002,0.002+aa.y,abs(distance-(radius-0.014)));
float vertical=(1-smoothstep(0.0014,0.0014+aa.x,abs(p.x)))*step(0.012,abs(p.y))*(1-step(0.18,abs(p.y)));
float horizontal=(1-smoothstep(0.0014,0.0014+aa.y,abs(p.y)))*step(0.012,abs(p.x))*(1-step(0.18,abs(p.x)));
float center=1-smoothstep(0.0025,0.0025+aa.y,distance);
float mark=max(center,max(vertical,horizontal));
float alpha=max(outside,max(rim*0.65,mark))*visibility;
float3 tint=float3(0.015,0.035,0.045)*outside+float3(0.08,0.28,0.36)*rim*0.65+float3(0.5,0.85,1)*mark;
color=float4(tint*visibility,alpha);
#endif
}
"#;

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    symbol: &str,
    inputs: &BTreeMap<String, u8>,
    reticle: Reticle,
) -> Result<Value> {
    // This native reticle's input two controls its existing appearance ramp.
    // Resolve the same named input on the selected sight, whose order can differ.
    let witness = native.tag(0x80ba71c2, Some(0x80809c36))?;
    let witness_inputs = crate::tiger::channel::object_channel_map(&witness.0)?;
    let name = witness_inputs
        .iter()
        .find_map(|(name, &index)| (index == 2).then_some(name))
        .context("Native reticle visibility input is absent")?;
    let input = *inputs
        .get(name)
        .context("Selected optic lacks reticle visibility")?;
    let material = native.tag(0x8152c441, Some(0x808071e8))?;
    ensure!(
        material.u32(24)? == 0x04002083 && material.u32(32)? == 0x88,
        "Native reticle rendering contract differs"
    );
    let mut bytes = material.0.clone();
    let code = material.array(0x2e8, 1, Some(0x80800009))?;
    let expected = hex::decode(
        "4c0049213f490c47264d022200232200370023220043033c5c0043063c5c0143073c5c0243083c5c0343093c5c04430a3c5c05430b3d49054312",
    )?;
    ensure!(
        code.len() == expected.len()
            && code
                .iter()
                .zip(&expected)
                .all(|(&at, &value)| material.0[at] == value),
        "Native reticle visibility program differs"
    );
    // The overlay has no sampled lens or stencil resources. Give its alpha the
    // selected optic's live aim value without retaining unused native bindings.
    for at in [0x2d0, 0x2f8, 0x308, 0x328] {
        put(&mut bytes, at, &[0; 16])?;
    }
    append(&mut bytes, 0x2e8, 0x80800009, &[0x4d, input, 0x43, 3], 1)?;
    bytes[0x33c] = 0x10;
    put(&mut bytes, 0x348, &0u32.to_le_bytes())?;
    put(&mut bytes, 0x34c, &u32::MAX.to_le_bytes())?;
    // State zero disables both depth and stencil. Rasterizer one is two-sided.
    // A screen-space overlay must not inherit the physical sight's clipping.
    bytes[0x21] = 0x80;
    bytes[0x22] = 0x81;
    put(
        &mut bytes,
        0x338,
        &u32::try_from(inputs.len())?.to_le_bytes(),
    )?;
    // The full-screen vertex positions need no view or object expression reads.
    for at in [0x50, 0x68, 0x78, 0x88, 0x98, 0xa8] {
        put(&mut bytes, at, &[0; 16])?;
    }
    put(&mut bytes, 0xb8, &0u32.to_le_bytes())?;
    bytes[0xbc] = 0;
    put(&mut bytes, 0xc8, &u32::MAX.to_le_bytes())?;
    let vertex = "scope-vertex";
    let pixel = "scope-pixel";
    crate::halo_reach::native_material::program(
        native,
        graph,
        vertex,
        material.u32(0x48)?,
        VERTEX,
        true,
    )?;
    crate::halo_reach::native_material::program(
        native,
        graph,
        pixel,
        material.u32(0x2c8)?,
        &format!(
            "#define RANGEFINDER {}\n{PIXEL}",
            u8::from(reticle == Reticle::Rangefinder)
        ),
        false,
    )?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    put(&mut bytes, 0x2c8, &u32::MAX.to_le_bytes())?;
    graph.add(
        "scope-material",
        0x8152c441,
        &bytes,
        None,
        vec![
            json!({"offset":0x48,"symbol":vertex}),
            json!({"offset":0x2c8,"symbol":pixel}),
        ],
    )?;

    let mesh = Mesh {
        positions: vec![[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.], [1., 1., 0.]],
        attributes: vec![[0., 0., 0., 0., 1., 1., 1., 0., 0., 1.]; 4],
        groups: vec![(0, vec![[0, 1, 2], [2, 1, 3]])],
        weights: Vec::new(),
        bones: 0,
    };
    let model = native.tag(0x80ba71be, Some(0x808073a5))?;
    let row = *model
        .array(16, 136, Some(0x80807378))?
        .first()
        .context("Native reticle mesh")?;
    let mut encoded =
        geometry::encode_source_passes(&mesh, &model, row, &BTreeMap::from([(0, 16)]))?;
    for (index, kind) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let tag = model.u32(row + [0, 4, 16][index])?;
        let mut header = native.tag(tag, None)?.0.clone();
        put(
            &mut header,
            if index == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[index].len())?.to_le_bytes(),
        )?;
        if index == 2 {
            header[1] = 0;
        } else {
            put(
                &mut header,
                4,
                &(if index == 0 { 8u16 } else { 24u16 }).to_le_bytes(),
            )?;
        }
        let header_symbol = format!("scope-{kind}-header");
        let data_symbol = format!("scope-{kind}-data");
        graph.add(&header_symbol, tag, &header, Some(&data_symbol), vec![])?;
        graph.add(
            &data_symbol,
            native.reference(tag)?,
            &encoded.streams[index],
            Some(&header_symbol),
            vec![],
        )?;
        let old = format!("{kind}-header");
        for patch in &mut encoded.patches {
            if patch["symbol"] == old {
                patch["symbol"] = json!(header_symbol);
            }
        }
    }
    for patch in &mut encoded.patches {
        if patch["symbol"] == "material-00000000" {
            patch["symbol"] = json!("scope-material");
        }
    }
    graph.add(symbol, 0x80ba71be, &encoded.header, None, encoded.patches)?;
    Ok(
        json!({"model":symbol,"material":"scope-material","stage":16,"visibility_input":name,"visibility_slot":input,
        "native_visibility_material":"8152C441","projection":"Full viewport with aspect-correct circular mask","reticle":reticle,
        "depth_stencil_state":0,"rasterizer_state":1,
        "physical_lenses_preserved":true,"gameplay_verified":false}),
    )
}
