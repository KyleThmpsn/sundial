use super::*;
use std::fmt::Write;

pub(in crate::model_preview) fn source(model: &Model, vertex: bool) -> String {
    let mut text = String::from(glsl::HELPERS);
    if vertex {
        text.push_str(VERTEX);
    } else {
        text.push_str(PIXEL);
        text.push_str(cube::GLSL);
    }
    if !vertex {
        sampling_source(model, &mut text);
    }
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        let program = if vertex {
            native
                .vertex
                .as_ref()
                .filter(|v| v.stored_uv.is_none())
                .map(|v| &v.code)
        } else {
            Some(&native.pixel)
        };
        if let Some(program) = program {
            text.push_str(&program.glsl(&format!("nProgram{index}")));
        }
    }
    if vertex {
        vertex_source(model, &mut text);
    } else {
        pixel_source(model, &mut text);
    }
    text
}

/// The pixel stage's resource reads: sampling, sizes and cube levels of detail.
fn sampling_source(model: &Model, text: &mut String) {
    sample_source(model, text);
    size_source(model, text);
    lod_source(model, text);
}

fn sample_source(model: &Model, text: &mut String) {
    text.push_str("uvec4 nSample(int resource,int samplerIndex,vec4 uv,float lod,bool explicitLod,ivec3 offset){\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        writeln!(text, "if(uNativeIndex=={index}){{").unwrap();
        for (unit, binding) in native.bindings.iter().enumerate() {
            sample_binding(text, unit, binding);
        }
        text.push_str("}\n");
    }
    text.push_str("return uvec4(0u);}\n");
}

/// One binding's read in `nSample`: a scene constant, nothing for depth and masks, or the
/// texture in texture unit `unit` with a neutral value when it is absent.
fn sample_binding(text: &mut String, unit: usize, binding: &Binding) {
    if matches!(binding.role, Role::Scene) {
        writeln!(
            text,
            "if(resource=={})return floatBitsToUint(vec4({}.0));",
            binding.slot,
            if binding.slot == 16 { 1 } else { 0 }
        )
        .unwrap();
        return;
    }
    if matches!(binding.role, Role::Depth | Role::Mask) {
        return;
    }
    let sampler = SAMPLERS[unit];
    let fallback = match binding.role {
        Role::Normal | Role::DetailNormal => "vec4(0.5,0.5,1.0,1.0)",
        Role::Detail => "vec4(0.25)",
        _ => "vec4(0.0)",
    };
    writeln!(
        text,
        "if(resource=={}){{if(uNativePresent[{unit}]==0)return floatBitsToUint({fallback});",
        binding.slot
    )
    .unwrap();
    if let Some(cube) = binding.cube {
        writeln!(
            text,
            "return floatBitsToUint(nCube({sampler},uv.xyz,lod,{},{}));}}",
            cube.edge, cube.levels
        )
        .unwrap();
    } else {
        // Match the software preview's base-level sampling. Native explicit cube
        // mips are preserved separately instead of being regenerated from an atlas.
        writeln!(text,"vec2 p=uv.xy+vec2(offset.xy)/vec2(textureSize({sampler},0));return floatBitsToUint(textureLod({sampler},p,0.0));}}").unwrap();
    }
}

fn size_source(model: &Model, text: &mut String) {
    text.push_str("uvec4 nSize(int resource){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(native) = &material.native {
            for (unit, binding) in native.bindings.iter().enumerate() {
                if matches!(binding.role, Role::Scene) {
                    writeln!(
                        text,
                        "if(uNativeIndex=={index}&&resource=={})return uvec4({}u);",
                        binding.slot,
                        if binding.slot == 15 { 0 } else { 1 }
                    )
                    .unwrap();
                } else if !matches!(binding.role, Role::Depth | Role::Mask) {
                    let sampler = SAMPLERS[unit];
                    let size = if let Some(cube) = binding.cube {
                        format!("uvec4({}u,{}u,0u,{}u)", cube.edge, cube.edge, cube.levels)
                    } else {
                        format!("uvec4(textureSize({sampler},0),0u,1u)")
                    };
                    writeln!(text,"if(uNativeIndex=={index}&&resource=={})return uNativePresent[{unit}]!=0?{size}:uvec4(0u);",binding.slot).unwrap();
                }
            }
        }
    }
    text.push_str("return uvec4(0u);}\n");
}

fn lod_source(model: &Model, text: &mut String) {
    text.push_str("vec4 nLod(int resource,vec4 direction){\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        for binding in &native.bindings {
            if let Some(cube) = binding.cube {
                writeln!(text,"if(uNativeIndex=={index}&&resource=={})return nCubeLod(direction.xyz,dFdx(direction.xyz),-dFdy(direction.xyz),{},{});",binding.slot,cube.edge,cube.levels).unwrap();
            }
        }
    }
    text.push_str("return vec4(0.0);}\n");
}

const SAMPLERS: [&str; 9] = [
    "uAlbedo",
    "uGear",
    "uNormal",
    "uDetail",
    "uDetailNormal",
    "uIridescence",
    "uEffectTexture0",
    "uEffectTexture1",
    "uEffectTexture2",
];

fn vertex_source(model: &Model, text: &mut String) {
    text.push_str("void nativeVertex(inout vec3 position,inout vec3 normal){\n");
    text.push_str("vNative[0]=vec4(normal,1.0);vNative[1]=aTangent;vNative[2]=vec4(cross(normal,aTangent.xyz)*aTangent.w,0.0);vNative[3]=vec4(aUv,aDetailUv);vNative[4]=vec4(position,1.0);vNative[5]=aColor;vNative[6]=vec4(0.0);vNative[7]=vec4(0.0);vNative[8]=aColor;\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(vertex) = material.native.as_ref().and_then(|n| n.vertex.as_ref()) else {
            continue;
        };
        if let Some(uv) = vertex.stored_uv {
            writeln!(
                text,
                "if(uNativeIndex=={index}){{vNative[3].xy=aUv*vec2({:.9},{:.9})+vec2({:.9},{:.9});",
                uv[0], uv[1], uv[2], uv[3]
            )
            .unwrap();
            if let Some(c) = vertex.stored_color {
                writeln!(
                    text,
                    "vNative[8]=vec4({:.9},{:.9},{:.9},{:.9});",
                    c[0], c[1], c[2], c[3]
                )
                .unwrap();
            }
            text.push_str("return;}\n");
            continue;
        }
        writeln!(
            text,
            "if(uNativeIndex=={index}){{vec4 v[16];vec4 o[16];for(int i=0;i<16;i++)v[i]=vec4(0.0);"
        )
        .unwrap();
        let has_weights = vertex.code.inputs.iter().any(|s| s.name == "BLENDWEIGHT");
        for semantic in &vertex.code.inputs {
            let value = match semantic.name.as_str() {
                "POSITION" => "vec4(position,0.0)",
                "NORMAL" => "vec4(normal,0.0)",
                "TANGENT" => "aTangent",
                "COLOR" => "aColor",
                "BLENDWEIGHT" => "vec4(1.0,0.0,0.0,0.0)",
                "BLENDINDICES" => {
                    if has_weights {
                        "vec4(0.0)"
                    } else {
                        "uintBitsToFloat(uvec4(0u,0u,255u,0u))"
                    }
                }
                "TEXCOORD" if semantic.index == 0 => "vec4(aUv,0.0,0.0)",
                _ => {
                    "vec4(abs(aUv.x)>1e-8?aDetailUv.x/aUv.x:1.0,abs(aUv.y)>1e-8?aDetailUv.y/aUv.y:1.0,0.0,0.0)"
                }
            };
            writeln!(text, "v[{}]={value};", semantic.register).unwrap();
        }
        writeln!(text, "nProgram{index}(v,o);").unwrap();
        for semantic in &vertex.code.outputs {
            if semantic.name == "TEXCOORD" && semantic.index < 9 {
                writeln!(
                    text,
                    "vNative[{}]=o[{}];",
                    semantic.index, semantic.register
                )
                .unwrap();
            }
        }
        text.push_str("position=vNative[4].xyz;normal=vNative[0].xyz;return;}\n");
    }
    text.push_str("}\n");
}

fn pixel_source(model: &Model, text: &mut String) {
    text.push_str(
        "vec4 nativePixel(){\nvec4 v[16];vec4 o[16];for(int i=0;i<16;i++)v[i]=vec4(0.0);\n",
    );
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        writeln!(text, "if(uNativeIndex=={index}){{").unwrap();
        if native.pixel.inputs.is_empty() {
            text.push_str("for(int i=0;i<9;i++)v[i]=vNative[i];\n");
        }
        for semantic in &native.pixel.inputs {
            let value=match semantic.system {
                1=>"vec4(gl_FragCoord.x,float(textureSize(uSceneDepth,0).y)-gl_FragCoord.y,0.0,1.0)".to_owned(),
                9=>"uintBitsToFloat(uvec4(gl_FrontFacing?0xFFFFFFFFu:0u))".to_owned(),
                _=>format!("vNative[{}]",semantic.index),
            };
            writeln!(text, "v[{}]={value};", semantic.register).unwrap();
        }
        writeln!(text,"nProgram{index}(v,o);vec4 result=vec4(max(o[0].rgb*uExposure,vec3(0.0)),clamp(o[0].a,0.0,1.0));if(any(isnan(result))||any(isinf(result)))return vec4(0.0);return result;}}").unwrap();
    }
    text.push_str("return vec4(0.0);}\n");
}

impl Native {
    pub(in crate::model_preview) fn quaternion(&self) -> bool {
        self.vertex.as_ref().is_some_and(|v| v.quaternion)
    }
}

const VERTEX: &str = r#"
uniform int uNativeIndex,uNativeQuaternion;
uniform vec4 uNativeVertexConstants[128];
vec4 nConstant(int buffer,int index){
    if(buffer==0)return uNativeVertexConstants[clamp(index,0,127)];
    if(buffer==11){
        if(index==5||index==7)return vec4(0.0,0.0,0.0,1.0);
        if(index==6)return vec4(1.0,1.0,0.0,0.0);
        if(uNativeQuaternion!=0){if(index==8)return vec4(0.0,0.0,0.0,1.0);return vec4(0.0);}
        if(index>=8&&index<=10){vec4 v=vec4(0.0);v[index-8]=1.0;return v;}
    }
    if(buffer==12&&index>=0&&index<4){vec4 v=vec4(0.0);v[index]=1.0;return v;}
    return vec4(0.0);
}
"#;

const PIXEL: &str = r#"
uniform int uNativeIndex;
uniform int uNativePresent[9];
uniform vec4 uNativeDye[27];
uniform vec3 uNativeDirection;
uniform float uNativeDistance;
vec4 nConstant(int buffer,int index){
    if(buffer==0)return uEffectConstants[clamp(index,0,127)];
    if(buffer>=5&&buffer<=7)return uNativeDye[clamp(index,0,26)];
    if(buffer==2&&index==0)return vec4(0.0,1.0,0.0,0.0);
    if(buffer==13&&index==1)return vec4(1.0);
    if(buffer==12){
        if(index==6)return vec4(uNativeDirection,0.0);
        if(index==7)return vec4(vNative[4].xyz+uNativeDirection*uNativeDistance,1.0);
        if(index==12){vec2 size=vec2(textureSize(uSceneDepth,0));return vec4(size,1.0/size);}
    }
    return vec4(0.0);
}
uvec4 nLoad(int resource,ivec4 position,ivec3 offset){
    ivec2 size=textureSize(uSceneDepth,0);
    ivec2 p=position.xy+offset.xy;
    bool inside=all(greaterThanEqual(p,ivec2(0)))&&all(lessThan(p,size));
    float depth=inside?texelFetch(uSceneDepth,ivec2(p.x,size.y-1-p.y),0).r:1.0;
    if(resource==3)return uvec4(depth<1.0?8u:0u);
    float gap=depth<1.0?max((depth-gl_FragCoord.z)*2.0/max(uDepthScale,1e-8),0.0):1e6;
    return floatBitsToUint(vec4(1.0/max(uNativeDistance+gap,1e-6)));
}
"#;
