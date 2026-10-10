use super::*;
use std::fmt::Write;

pub(in crate::model_preview) fn source(model: &Model, vertex: bool) -> String {
    let mut text = String::from(glsl::HELPERS);
    if vertex {
        text.push_str(VERTEX);
    } else {
        text.push_str(PIXEL);
    }
    text.push_str(cube::GLSL);
    text.push_str(layered::GLSL);
    sampling_source(model, &mut text, vertex);
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
        paint_source(model, &mut text);
        normal_source(model, &mut text);
        grain_source(model, &mut text);
        pixel_source(model, &mut text);
        surface_source(model, &mut text);
    }
    text
}

/// The pixel stage's resource reads: sampling, sizes and cube levels of detail.
fn sampling_source(model: &Model, text: &mut String, vertex: bool) {
    sample_source(model, text, vertex);
    size_source(model, text, vertex);
    load_source(model, text, vertex);
    lod_source(model, text, vertex);
}

fn sample_source(model: &Model, text: &mut String, vertex: bool) {
    text.push_str("uvec4 nSample(int resource,int samplerIndex,vec4 uv,float lod,int sampling,vec4 dx,vec4 dy,ivec3 offset){\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        writeln!(text, "if(uNativeIndex=={index}){{").unwrap();
        for (unit, binding) in native.bindings.iter().enumerate() {
            if binding.vertex != vertex {
                continue;
            }
            sample_binding(
                text,
                unit,
                native.texture_unit(unit),
                binding,
                binding.sampler.and_then(|i| material.samplers.get(i)),
            );
        }
        text.push_str("}\n");
    }
    text.push_str("return uvec4(0u);}\n");
}

/// One binding's read in `nSample`: a scene constant, nothing for depth and masks, or the
/// texture in texture unit `unit` with a neutral value when it is absent.
fn sample_binding(
    text: &mut String,
    unit: usize,
    texture: usize,
    binding: &Binding,
    settings: Option<&texture::Sampler>,
) {
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
    let sampler = SAMPLERS[texture];
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
            "if(sampling==2)lod=nCubeLod(uv.xyz,dx.xyz,dy.xyz,{},{}).y;",
            cube.edge, cube.levels
        )
        .unwrap();
        if let Some(settings) = settings.filter(|s| s.filter.is_some()) {
            writeln!(
                text,
                "if(sampling==2)lod=clamp(lod+{:.9},{:.9},{:.9});",
                settings.mip_bias, settings.lod[0], settings.lod[1]
            )
            .unwrap();
        }
        writeln!(
            text,
            "return floatBitsToUint(nCube({sampler},uv.xyz,lod,{},{}));}}",
            cube.edge, cube.levels
        )
        .unwrap();
    } else if let Some(image) = binding.layered {
        sample_layered(text, sampler, image, settings);
    } else {
        sample_image(text, sampler, unit, settings);
    }
}

fn layer_arguments(image: layered::Layered) -> String {
    format!(
        "ivec3({},{},{}),{},{},{}",
        image.size[0], image.size[1], image.size[2], image.levels, image.columns, image.volume
    )
}

fn sample_layered(
    text: &mut String,
    sampler: &str,
    image: layered::Layered,
    settings: Option<&texture::Sampler>,
) {
    let settings = settings.copied().unwrap_or_default();
    let address = |mode| match mode {
        texture::AddressMode::Wrap => 1,
        texture::AddressMode::Mirror => 2,
        texture::AddressMode::Clamp => 3,
        texture::AddressMode::Border => 4,
        texture::AddressMode::MirrorOnce => 5,
    };
    let size = format!(
        "ivec3({},{},{})",
        image.size[0], image.size[1], image.size[2]
    );
    writeln!(text, "vec2 direction=vec2(0.0);int taps=1;if(sampling==2){{lod=nLayerLod({size},{},dx.xyz,dy.xyz);", image.volume).unwrap();
    if !image.volume && settings.anisotropy > 1 {
        writeln!(text,"vec2 size=vec2({}.0,{}.0),x=dx.xy*size,y=dy.xy*size;float a=x.x*x.x+y.x*y.x,b=x.x*x.y+y.x*y.y,d=x.y*x.y+y.y*y.y;float delta=length(vec2(a-d,2.0*b));float major=sqrt(max((a+d+delta)*0.5,0.0));float minor=max(sqrt(max((a+d-delta)*0.5,0.0)),major/{}.0);taps=int(clamp(ceil(major/max(minor,1e-8)),1.0,{}.0));float angle=0.5*atan(2.0*b,a-d);direction=vec2(cos(angle),sin(angle))*major/size;lod=log2(max(minor,1e-20));", image.size[0], image.size[1], settings.anisotropy, settings.anisotropy).unwrap();
    }
    writeln!(text, "}}lod+={:.9};vec4 result=vec4(0.0);for(int tap=0;tap<16;tap++){{if(tap>=taps)break;vec3 p=uv.xyz;p.xy+=direction*((float(tap)+0.5)/float(taps)-0.5);", settings.mip_bias).unwrap();
    let border = settings.border.map(|v| v / 255.0);
    writeln!(text, "result+=nLayerSample({sampler},{},p,lod,offset,ivec3({},{},{}),vec4({:.9},{:.9},{:.9},{:.9}),{},vec2({:.9},{:.9}))/float(taps);}}return floatBitsToUint(result);}}",
        layer_arguments(image), address(settings.u), address(settings.v), address(settings.w),
        border[0], border[1], border[2], border[3], settings.filter.unwrap_or(21), settings.lod[0], settings.lod[1]).unwrap();
}

fn sample_image(
    text: &mut String,
    sampler: &str,
    unit: usize,
    settings: Option<&texture::Sampler>,
) {
    let Some(settings) = settings.filter(|s| s.filter.is_some()) else {
        writeln!(text,"vec2 p=uv.xy+vec2(offset.xy)/vec2(textureSize({sampler},0));return floatBitsToUint(textureLod({sampler},p,0.0));}}").unwrap();
        return;
    };
    writeln!(text,"vec2 size=vec2(textureSize({sampler},0));vec2 direction=vec2(0.0);int taps=1;if(sampling==2){{vec2 x=dx.xy*size,y=dy.xy*size;float radius=max(length(x),length(y));").unwrap();
    if settings.anisotropy > 1 {
        writeln!(text,"float a=x.x*x.x+y.x*y.x,b=x.x*x.y+y.x*y.y,d=x.y*x.y+y.y*y.y;float delta=length(vec2(a-d,2.0*b));float major=sqrt(max((a+d+delta)*0.5,0.0));float minor=max(sqrt(max((a+d-delta)*0.5,0.0)),major/{}.0);taps=int(clamp(ceil(major/max(minor,1e-8)),1.0,{}.0));float angle=0.5*atan(2.0*b,a-d);direction=vec2(cos(angle),sin(angle))*major/size;radius=minor;",settings.anisotropy,settings.anisotropy).unwrap();
    }
    text.push_str("lod=log2(max(radius,1e-20));if(isnan(lod)||isinf(lod)||any(isnan(direction))||any(isinf(direction))){lod=0.0;direction=vec2(0.0);taps=1;}}\n");
    writeln!(
        text,
        "lod=clamp(lod+{:.9},{:.9},{:.9});lod=clamp(lod,0.0,float(uNativePresent[{unit}]-1));",
        settings.mip_bias, settings.lod[0], settings.lod[1]
    )
    .unwrap();
    let (low, high, blend) = if settings.filter.unwrap() & 1 != 0 {
        ("floor(lod)", "ceil(lod)", "fract(lod)")
    } else {
        ("floor(lod+0.5)", "floor(lod+0.5)", "0.0")
    };
    writeln!(text,"float low={low},high={high},blend={blend};vec4 result=vec4(0.0);for(int tap=0;tap<16;tap++){{if(tap>=taps)break;vec2 p=uv.xy+direction*((float(tap)+0.5)/float(taps)-0.5);vec2 a=p+vec2(offset.xy)/vec2(textureSize({sampler},int(low)));vec2 b=p+vec2(offset.xy)/vec2(textureSize({sampler},int(high)));result+=mix(textureLod({sampler},a,low),textureLod({sampler},b,high),blend)/float(taps);}}return floatBitsToUint(result);}}").unwrap();
}

fn size_source(model: &Model, text: &mut String, vertex: bool) {
    text.push_str("uvec4 nSize(int resource,int mip){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(native) = &material.native {
            for (unit, binding) in native.bindings.iter().enumerate() {
                if binding.vertex != vertex {
                    continue;
                }
                if matches!(binding.role, Role::Scene) {
                    writeln!(
                        text,
                        "if(uNativeIndex=={index}&&resource=={})return uvec4({}u);",
                        binding.slot,
                        if binding.slot == 15 { 0 } else { 1 }
                    )
                    .unwrap();
                } else if !matches!(binding.role, Role::Depth | Role::Mask) {
                    let sampler = SAMPLERS[native.texture_unit(unit)];
                    let size = if let Some(cube) = binding.cube {
                        format!(
                            "uvec4(mip>=0&&mip<{}?max({}>>mip,1):0,mip>=0&&mip<{}?max({}>>mip,1):0,0,{})",
                            cube.levels, cube.edge, cube.levels, cube.edge, cube.levels
                        )
                    } else if let Some(image) = binding.layered {
                        format!(
                            "uvec4(mip>=0&&mip<{}?nLayerSize(ivec3({},{},{}),mip,{}):ivec3(0,0,{}),{})",
                            image.levels,
                            image.size[0],
                            image.size[1],
                            image.size[2],
                            image.volume,
                            if image.volume { 0 } else { image.size[2] },
                            image.levels
                        )
                    } else {
                        format!(
                            "uvec4(mip>=0&&mip<uNativePresent[{unit}]?textureSize({sampler},mip):ivec2(0),0,uNativePresent[{unit}])"
                        )
                    };
                    writeln!(text,"if(uNativeIndex=={index}&&resource=={})return uNativePresent[{unit}]!=0?{size}:uvec4(0u);",binding.slot).unwrap();
                }
            }
        }
    }
    text.push_str("return uvec4(0u);}\n");
}

fn load_source(model: &Model, text: &mut String, vertex: bool) {
    text.push_str("uvec4 nLoad(int resource,ivec4 position,ivec3 offset){\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        for (unit, binding) in native.bindings.iter().enumerate() {
            if binding.vertex != vertex {
                continue;
            }
            writeln!(
                text,
                "if(uNativeIndex=={index}&&resource=={}){{",
                binding.slot
            )
            .unwrap();
            if matches!(binding.role, Role::Mask | Role::Depth) {
                text.push_str("return nSceneLoad(resource,position,offset);}\n");
                continue;
            }
            if matches!(binding.role, Role::Scene) || binding.cube.is_some() {
                text.push_str("return uvec4(0u);}\n");
                continue;
            }
            let sampler = SAMPLERS[native.texture_unit(unit)];
            writeln!(text, "if(uNativePresent[{unit}]==0)return uvec4(0u);").unwrap();
            if let Some(image) = binding.layered {
                writeln!(
                    text,
                    "return floatBitsToUint(nLayerLoad({sampler},{},position,offset));}}",
                    layer_arguments(image)
                )
                .unwrap();
            } else {
                writeln!(text, "if(position.w<0||position.w>=uNativePresent[{unit}])return uvec4(0u);ivec2 p=position.xy+offset.xy;ivec2 size=textureSize({sampler},position.w);if(any(lessThan(p,ivec2(0)))||any(greaterThanEqual(p,size)))return uvec4(0u);return floatBitsToUint(texelFetch({sampler},p,position.w));}}").unwrap();
            }
        }
    }
    text.push_str("return uvec4(0u);}\n");
}

fn lod_source(model: &Model, text: &mut String, vertex: bool) {
    text.push_str("vec4 nLod(int resource,vec4 direction){\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        for binding in &native.bindings {
            if vertex || binding.vertex {
                continue;
            }
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
        if let Some(uv) = material.native.as_ref().and_then(|n| n.opaque_uv) {
            writeln!(
                text,
                "if(uNativeIndex=={index}){{vNative[3].xy=aUv*vec2({},{})+vec2({},{});return;}}",
                uv[0], uv[1], uv[2], uv[3]
            )
            .unwrap();
            continue;
        }
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

fn paint_source(model: &Model, text: &mut String) {
    text.push_str("bool nHasPaint(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if material
            .native
            .as_ref()
            .is_some_and(|native| native.paint.is_some())
        {
            writeln!(text, "if(uNativeIndex=={index})return true;").unwrap();
        }
    }
    text.push_str("return false;}\nvec2 nPaintSmooth(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(paint) = material
            .native
            .as_ref()
            .and_then(|native| native.paint.as_ref())
        {
            writeln!(text, "if(uNativeIndex=={index})return {};", paint.glsl()).unwrap();
        }
    }
    text.push_str("return vec2(0.0);}\n");
    text.push_str("bool nHasBaseMetal(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if material
            .native
            .as_ref()
            .and_then(|native| native.paint.as_ref())
            .and_then(|p| p.metal_glsl())
            .is_some()
        {
            writeln!(text, "if(uNativeIndex=={index})return true;").unwrap();
        }
    }
    text.push_str("return false;}\nfloat nBaseMetal(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(value) = material
            .native
            .as_ref()
            .and_then(|native| native.paint.as_ref())
            .and_then(|p| p.metal_glsl())
        {
            writeln!(text, "if(uNativeIndex=={index})return {value};").unwrap();
        }
    }
    text.push_str("return 0.0;}\n");
}

fn normal_source(model: &Model, text: &mut String) {
    text.push_str("bool nHasDecodedNormal(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(value) = material
            .native
            .as_ref()
            .and_then(|n| n.paint.as_ref())
            .and_then(|p| p.normal_valid_glsl())
        {
            writeln!(text, "if(uNativeIndex=={index})return {value};").unwrap();
        }
    }
    text.push_str("return false;}\nvec4 nNormalDecode(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(value) = material
            .native
            .as_ref()
            .and_then(|n| n.paint.as_ref())
            .and_then(|p| p.normal_glsl())
        {
            writeln!(text, "if(uNativeIndex=={index})return {value};").unwrap();
        }
    }
    text.push_str("return vec4(2.0,-1.0,2.0,-1.0);}\n");
}

fn grain_source(model: &Model, text: &mut String) {
    text.push_str("bool nHasNormalGrain(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(value) = material
            .native
            .as_ref()
            .and_then(|n| n.paint.as_ref())
            .and_then(|p| p.grain_valid_glsl())
        {
            writeln!(text, "if(uNativeIndex=={index})return {value};").unwrap();
        }
    }
    text.push_str("return false;}\nfloat nNormalGrain(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if let Some(value) = material
            .native
            .as_ref()
            .and_then(|n| n.paint.as_ref())
            .and_then(|p| p.grain_glsl())
        {
            writeln!(text, "if(uNativeIndex=={index})return {value};").unwrap();
        }
    }
    text.push_str("return 0.0;}\n");
}

fn surface_source(model: &Model, text: &mut String) {
    text.push_str("bool nativeSurface(out vec3 albedo,out vec4 normal,out float metal,out float ambient,out vec3 emission,out float coverage){albedo=vec3(0.0);normal=vec4(0.0,0.0,1.0,1.0);metal=0.0;ambient=1.0;emission=vec3(0.0);coverage=1.0;vec4 v[16];vec4 o[16];for(int i=0;i<16;i++)v[i]=vec4(0.0);\n");
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = material.native.as_ref().filter(|n| n.deferred) else {
            continue;
        };
        writeln!(text, "if(uNativeIndex=={index}){{").unwrap();
        pixel_inputs(text, &native.pixel);
        writeln!(text,"nProgram{index}(v,o);if(any(isnan(o[0]))||any(isinf(o[0]))||any(isnan(o[1]))||any(isinf(o[1]))||any(isnan(o[2]))||any(isinf(o[2])))return false;").unwrap();
        if native.decal {
            text.push_str("coverage=clamp(1.0-o[0].w,0.0,1.0);if(coverage<=1e-8){coverage=0.0;return true;}o[0].rgb/=coverage;if(all(equal(o[1],vec4(0.0)))){o[1]=vec4(normalize(vNative[0].xyz)*0.375+vec3(0.5),0.0);}else{float alpha=clamp(1.0-o[1].w,0.0,1.0);if(alpha<=1e-8){coverage=0.0;return true;}o[1].rgb/=alpha;}o[2]=vec4(0.0,0.5,0.0,1.0);\n");
        }
        text.push_str("vec3 encoded=o[1].xyz-vec3(0.5);float radius=length(encoded);if(radius<1e-8)return false;albedo=max(o[0].rgb,vec3(0.0));normal=vec4(encoded/radius,1.0-clamp((radius-0.375)*8.0,0.0,1.0));metal=clamp(o[2].x,0.0,1.0);\n");
        if let Some(power) = native.ambient_power {
            writeln!(text, "ambient=nAmbient(o[2].yw,{power:.9});").unwrap();
        } else {
            text.push_str("ambient=clamp(2.0*o[2].y,0.0,1.0)*clamp(o[2].w,0.0,1.0);\n");
        }
        if native.intensity {
            text.push_str("emission=albedo*nIntensity(o[2].y);\n");
        }
        text.push_str("return true;}\n");
    }
    text.push_str("return false;}\n");
}

fn pixel_inputs(text: &mut String, program: &program::Program) {
    if program.inputs.is_empty() {
        text.push_str("for(int i=0;i<9;i++)v[i]=vNative[i];\n");
    }
    for semantic in &program.inputs {
        let value = match semantic.system {
            1 => "vec4(gl_FragCoord.x,float(textureSize(uSceneDepth,0).y)-gl_FragCoord.y,0.0,1.0)"
                .to_owned(),
            9 => "uintBitsToFloat(uvec4(gl_FrontFacing?0xFFFFFFFFu:0u))".to_owned(),
            _ => format!("vNative[{}]", semantic.index),
        };
        writeln!(text, "v[{}]={value};", semantic.register).unwrap();
    }
}

fn pixel_source(model: &Model, text: &mut String) {
    text.push_str("bool nHasIntensity(){\n");
    for (index, material) in model.effects.iter().enumerate() {
        if material
            .native
            .as_ref()
            .is_some_and(|native| native.intensity)
        {
            writeln!(text, "if(uNativeIndex=={index})return true;").unwrap();
        }
    }
    text.push_str("return false;}\nfloat nIntensity(float value){float y=roundEven(clamp(value,0.0,1.0)*255.0)/255.0;return exp2(13.0*clamp(2.0*y-(1.0+2.0/255.0),0.0,1.0)-7.0)-1.0/128.0;}\n");
    text.push_str("float nAmbient(vec2 value,float power){vec2 quantized=roundEven(clamp(value,vec2(0.0),vec2(1.0))*255.0)/255.0;float visibility=clamp(2.0*quantized.x,0.0,1.0)*quantized.y;return pow(max(visibility*visibility,0.0001),power);}\n");
    text.push_str(
        "vec4 nativePixel(vec3 gearBase,out float ambient){\nambient=-1.0;vec4 v[16];vec4 o[16];for(int i=0;i<16;i++)v[i]=vec4(0.0);\n",
    );
    for (index, material) in model.effects.iter().enumerate() {
        let Some(native) = &material.native else {
            continue;
        };
        writeln!(text, "if(uNativeIndex=={index}){{").unwrap();
        pixel_inputs(text, &native.pixel);
        if native.opaque() {
            if let Some(gain) = &native.base_gain {
                writeln!(
                    text,
                    "if(uHasGear==1 && samplePlate(uGear,vUv,1).a<40.0/255.0)gearBase*={};",
                    gain.glsl()
                )
                .unwrap();
            }
            text.push_str("v[15]=vec4(gearBase,1.0);\n");
        }
        let exposure = if native.opaque() { "1.0" } else { "uExposure" };
        let alpha = if native.intensity {
            "nIntensity(o[2].y)"
        } else if native.opaque() {
            "1.0"
        } else {
            "clamp(o[0].a,0.0,1.0)"
        };
        writeln!(text,"nProgram{index}(v,o);vec4 result=vec4(max(o[0].rgb*{exposure},vec3(0.0)),{alpha});if(any(isnan(result))||any(isinf(result)))return vec4(0.0);").unwrap();
        if let Some(power) = native.ambient_power {
            writeln!(text,"if(!any(isnan(o[2].yw))&&!any(isinf(o[2].yw)))ambient=nAmbient(o[2].yw,{power:.9});").unwrap();
        }
        text.push_str("return result;}\n");
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
uniform int uNativePresent[9];
uniform sampler2D uAlbedo,uGear,uNormal,uDetail,uDetailNormal,uIridescence,uEffectTexture0,uEffectTexture1,uEffectTexture2;
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
uvec4 nSceneLoad(int resource,ivec4 position,ivec3 offset){
    ivec2 size=textureSize(uSceneDepth,0);
    ivec2 p=position.xy+offset.xy;
    bool inside=all(greaterThanEqual(p,ivec2(0)))&&all(lessThan(p,size));
    float depth=inside?texelFetch(uSceneDepth,ivec2(p.x,size.y-1-p.y),0).r:1.0;
    if(resource==3)return uvec4(depth<1.0?8u:0u);
    float gap=depth<1.0?max((depth-gl_FragCoord.z)*2.0/max(uDepthScale,1e-8),0.0):1e6;
    return floatBitsToUint(vec4(1.0/max(uNativeDistance+gap,1e-6)));
}
"#;
