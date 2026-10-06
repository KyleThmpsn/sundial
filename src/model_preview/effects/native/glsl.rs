//! Emit the same decoded register operations used by the software evaluator.
use super::program::{Operand, Program};
use std::fmt::Write;

fn index(value: &Operand, axis: usize) -> String {
    let index = &value.indices[axis];
    match &index.relative {
        Some(relative) => format!("int({}u + ({}).x)", index.base, bits(relative)),
        None => index.base.to_string(),
    }
}

fn bits(value: &Operand) -> String {
    let raw = match value.kind {
        0 => format!("r[{}]", index(value, 0)),
        1 => format!("floatBitsToUint(v[{}])", index(value, 0)),
        2 => format!("o[{}]", index(value, 0)),
        4 => format!(
            "uvec4({}u,{}u,{}u,{}u)",
            value.literal[0], value.literal[1], value.literal[2], value.literal[3]
        ),
        8 => format!(
            "floatBitsToUint(nConstant({},{}))",
            index(value, 0),
            index(value, 1)
        ),
        _ => "uvec4(0u)".into(),
    };
    let swizzle: String = value.lanes.iter().map(|&i| b"xyzw"[i] as char).collect();
    let raw = format!("({raw}).{swizzle}");
    match value.modifier {
        1 => format!("({raw} ^ uvec4(0x80000000u))"),
        2 => format!("({raw} & uvec4(0x7FFFFFFFu))"),
        3 => format!("({raw} | uvec4(0x80000000u))"),
        _ => raw,
    }
}

fn write(output: &mut String, destination: &Operand, value: &str, saturated: bool) {
    if destination.kind == 13 {
        return;
    }
    let name = if destination.kind == 0 { "r" } else { "o" };
    let mask: String = (0..4)
        .filter(|i| destination.mask & (1 << i) != 0)
        .map(|i| b"xyzw"[i] as char)
        .collect();
    let value = if saturated {
        format!("floatBitsToUint(nSaturate(uintBitsToFloat({value})))")
    } else {
        value.to_owned()
    };
    writeln!(
        output,
        "{name}[{}].{mask}=({value}).{mask};",
        index(destination, 0)
    )
    .unwrap();
}

impl Program {
    pub(super) fn glsl(&self, name: &str) -> String {
        let mut output = format!(
            "void {name}(in vec4 v[16],out vec4 result[16]){{\nuvec4 r[32]; uvec4 o[16];\nfor(int i=0;i<32;i++)r[i]=uvec4(0u);\nfor(int i=0;i<16;i++)o[i]=uvec4(0u);\n"
        );
        for instruction in &self.instructions {
            let v = &instruction.operands;
            let b = |i| bits(&v[i]);
            let f = |i| format!("uintBitsToFloat({})", b(i));
            let wrap = |s: String| format!("floatBitsToUint({s})");
            let value = match instruction.code {
                31 | 13 => {
                    let condition = format!(
                        "({}).x {} 0u",
                        b(0),
                        if instruction.nonzero { "!=" } else { "==" }
                    );
                    if instruction.code == 13 {
                        writeln!(output, "if({condition}) discard;").unwrap();
                    } else {
                        writeln!(output, "if({condition}){{").unwrap();
                    }
                    continue;
                }
                18 => {
                    output.push_str("}else{\n");
                    continue;
                }
                21 => {
                    output.push_str("}\n");
                    continue;
                }
                62 => {
                    output.push_str(
                        "for(int i=0;i<16;i++)result[i]=uintBitsToFloat(o[i]);\nreturn;\n",
                    );
                    continue;
                }
                0 => wrap(format!("{}+{}", f(1), f(2))),
                1 => format!("{}&{}", b(1), b(2)),
                14 => wrap(format!("{}/{}", f(1), f(2))),
                code @ 15..=17 => {
                    let mask = &"xyzw"[..(code - 13) as usize];
                    wrap(format!("vec4(dot(({}).{mask},({}).{mask}))", f(1), f(2)))
                }
                25 => wrap(format!("exp2({})", f(1))),
                26 => wrap(format!("fract({})", f(1))),
                27 => format!("nInteger({},true)", f(1)),
                28 => format!("nInteger({},false)", f(1)),
                code @ (29 | 49 | 57) => format!(
                    "uvec4({}({},{}))*0xFFFFFFFFu",
                    match code {
                        29 => "greaterThanEqual",
                        49 => "lessThan",
                        _ => "notEqual",
                    },
                    f(1),
                    f(2)
                ),
                30 => format!("{}+{}", b(1), b(2)),
                35 => format!("{}*{}+{}", b(1), b(2), b(3)),
                38 => {
                    // Audited skinning uses only the low product. Reject a live high result
                    // during contract validation instead of relying on optional GLSL int64.
                    write(&mut output, &v[1], &format!("{}*{}", b(2), b(3)), false);
                    continue;
                }
                32 => format!("uvec4(equal({},{}))*0xFFFFFFFFu", b(1), b(2)),
                41 => format!("{}<<({}&uvec4(31u))", b(1), b(2)),
                43 => wrap(format!("vec4(ivec4({}))", b(1))),
                45 => {
                    let swizzle: String = v[2].lanes.iter().map(|&i| b"xyzw"[i] as char).collect();
                    format!(
                        "nLoad({},ivec4({}),ivec3({},{},{})).{swizzle}",
                        index(&v[2], 0),
                        b(1),
                        instruction.offset[0],
                        instruction.offset[1],
                        instruction.offset[2]
                    )
                }
                47 => wrap(format!("log2({})", f(1))),
                50 => wrap(format!("{}*{}+{}", f(1), f(2), f(3))),
                51 | 52 => wrap(format!(
                    "{}({},{})",
                    if instruction.code == 51 {
                        "nMin"
                    } else {
                        "nMax"
                    },
                    f(1),
                    f(2)
                )),
                54 => b(1),
                55 => format!("nSelect({},{},{})", b(1), b(2), b(3)),
                56 => wrap(format!("{}*{}", f(1), f(2))),
                code @ 64..=68 => wrap(format!(
                    "{}({})",
                    match code {
                        64 => "roundEven",
                        65 => "floor",
                        66 => "ceil",
                        67 => "trunc",
                        _ => "inversesqrt",
                    },
                    f(1)
                )),
                60 => format!("({} | {})", b(1), b(2)),
                69 | 72 | 73 => {
                    let lod = if instruction.code == 72 {
                        format!("({}).x", f(4))
                    } else {
                        "0.0".into()
                    };
                    let swizzle: String = v[2].lanes.iter().map(|&i| b"xyzw"[i] as char).collect();
                    format!(
                        "nSample({},{},{},{},{},ivec3({},{},{})).{swizzle}",
                        index(&v[2], 0),
                        index(&v[3], 0),
                        f(1),
                        lod,
                        if instruction.code == 72 {
                            "true"
                        } else {
                            "false"
                        },
                        instruction.offset[0],
                        instruction.offset[1],
                        instruction.offset[2]
                    )
                }
                61 => {
                    let swizzle: String = v[2].lanes.iter().map(|&i| b"xyzw"[i] as char).collect();
                    format!("nSize({}).{swizzle}", index(&v[2], 0))
                }
                75 => wrap(format!("sqrt({})", f(1))),
                77 => {
                    // Either destination may reuse the source register.
                    writeln!(output, "{{vec4 angle={};", f(2)).unwrap();
                    write(
                        &mut output,
                        &v[0],
                        &wrap("sin(angle)".into()),
                        instruction.saturate,
                    );
                    write(
                        &mut output,
                        &v[1],
                        &wrap("cos(angle)".into()),
                        instruction.saturate,
                    );
                    output.push_str("}\n");
                    continue;
                }
                78 => {
                    writeln!(
                        output,
                        "{{uvec4 dividend={},divisor={};uvec4 quotient,remainder;",
                        b(2),
                        b(3)
                    )
                    .unwrap();
                    output.push_str("for(int lane=0;lane<4;lane++){quotient[lane]=divisor[lane]==0u?0xFFFFFFFFu:dividend[lane]/divisor[lane];remainder[lane]=divisor[lane]==0u?0xFFFFFFFFu:dividend[lane]%divisor[lane];}\n");
                    write(&mut output, &v[0], "quotient", false);
                    write(&mut output, &v[1], "remainder", false);
                    output.push_str("}\n");
                    continue;
                }
                80 => format!("uvec4(greaterThanEqual({},{}))*0xFFFFFFFFu", b(1), b(2)),
                86 => wrap(format!("vec4({})", b(1))),
                108 => {
                    let swizzle: String = v[2].lanes.iter().map(|&i| b"xyzw"[i] as char).collect();
                    wrap(format!("nLod({},{}).{swizzle}", index(&v[2], 0), f(1)))
                }
                122 | 124 => wrap(format!(
                    "{}({})",
                    if instruction.code == 122 {
                        "dFdx"
                    } else {
                        "-dFdy"
                    },
                    f(1)
                )),
                140 => format!("nInsert({},{},{},{})", b(1), b(2), b(3), b(4)),
                _ => unreachable!("validated shader instruction"),
            };
            write(&mut output, &v[0], &value, instruction.saturate);
        }
        output.push_str("}\n");
        output
    }
}

pub(super) const HELPERS: &str = r#"
vec4 nSaturate(vec4 v){
    for(int i=0;i<4;i++)v[i]=isnan(v[i])?0.0:clamp(v[i],0.0,1.0);
    return v;
}
vec4 nMin(vec4 a,vec4 b){
    for(int i=0;i<4;i++)a[i]=isnan(a[i])?b[i]:isnan(b[i])?a[i]:min(a[i],b[i]);
    return a;
}
vec4 nMax(vec4 a,vec4 b){
    for(int i=0;i<4;i++)a[i]=isnan(a[i])?b[i]:isnan(b[i])?a[i]:max(a[i],b[i]);
    return a;
}
uvec4 nInteger(vec4 v,bool signedValue){
    uvec4 result=uvec4(0u);
    for(int i=0;i<4;i++){
        if(isnan(v[i]))continue;
        if(signedValue){
            if(v[i]>=2147483648.0)result[i]=0x7FFFFFFFu;
            else if(v[i]<=-2147483648.0)result[i]=0x80000000u;
            else result[i]=uint(int(v[i]));
        }else{
            if(v[i]>=4294967296.0)result[i]=0xFFFFFFFFu;
            else if(v[i]>0.0)result[i]=uint(v[i]);
        }
    }
    return result;
}
uvec4 nSelect(uvec4 test,uvec4 a,uvec4 b){
    return uvec4(test.x!=0u?a.x:b.x,test.y!=0u?a.y:b.y,test.z!=0u?a.z:b.z,test.w!=0u?a.w:b.w);
}
uvec4 nInsert(uvec4 width,uvec4 offset,uvec4 insert,uvec4 base){
    uvec4 o=offset&uvec4(31u);
    uvec4 mask=((uvec4(1u)<<(width&uvec4(31u)))-uvec4(1u))<<o;
    return ((insert<<o)&mask)|(base&~mask);
}
"#;
