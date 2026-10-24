//! Material mapping curves use Reach's CPU evaluator, including its quantized periodic tables.
//! Named object inputs need a material caller binding before they can drive a converted surface.
use super::*;
use crate::halo_reach::effect::property::Property;

// Preserve MCC's division before wrapping the phase. Reciprocal multiplication can round to
// the other side of a period boundary and select the opposite end of a discontinuous curve.
pub(super) const CLOCK: &str =
    "precise float reachPhase(float time,float period) { return time/period; }\n";

#[derive(Default)]
pub(super) struct Parameters {
    names: BTreeMap<String, String>,
    pub source: String,
    pub notes: Vec<String>,
}

pub(super) fn vector(value: [f32; 4]) -> String {
    format!(
        "float4({:e},{:e},{:e},{:e})",
        value[0], value[1], value[2], value[3]
    )
}

impl Parameters {
    pub fn bindings(&self) -> &BTreeMap<String, String> {
        &self.names
    }
    pub fn new(
        material: &Material,
        constants: &BTreeMap<String, [f32; 4]>,
        overridden: &impl Fn(&str) -> bool,
    ) -> Self {
        let mut result = Self::default();
        let Some(property) = material.properties.first() else {
            return result;
        };
        for (name, overlays) in &property.overlays {
            if overridden(name) {
                continue;
            }
            let Some(&stored) = constants.get(name) else {
                continue;
            };
            let variable = format!("reachParameter{}", result.names.len());
            let mut assignments = String::new();
            for &index in overlays {
                let function = &property.functions[index];
                let (destination, source) = match function.kind {
                    0 | 3 => ("x", "x"),
                    1 => ("xyz", "xyz"),
                    2 => ("xy", "xx"),
                    4 => ("y", "x"),
                    5 => ("z", "x"),
                    6 | 8 => ("w", "x"),
                    7 => continue, // Frame indices are bound by the texture caller.
                    other => {
                        result.notes.push(format!(
                            "{name} overlay {index} has unsupported kind {other}"
                        ));
                        continue;
                    }
                };
                match expression(function) {
                    Ok(value) => assignments
                        .push_str(&format!("{variable}.{destination}=({value}).{source};\n")),
                    Err(error) => result.notes.push(format!(
                        "{name} overlay {index} keeps its stored value: {error:#}"
                    )),
                }
            }
            if !assignments.is_empty() {
                result.source.push_str(&format!(
                    "float4 {variable}={};\n{assignments}",
                    vector(stored)
                ));
                result.names.insert(name.clone(), variable);
            }
        }
        result
    }

    pub fn value(&self, name: &str, stored: [f32; 4]) -> String {
        self.names
            .get(name)
            .cloned()
            .unwrap_or_else(|| vector(stored))
    }

    pub fn unused(&self, shader: &str) -> Vec<String> {
        let consumers = shader.replace(&self.source, "");
        let tokens = consumers
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .collect::<std::collections::BTreeSet<_>>();
        self.names
            .iter()
            .filter(|(_, variable)| !tokens.contains(variable.as_str()))
            .map(|(name, _)| format!("Animated parameter {name} has no translated shader consumer"))
            .collect()
    }
}

pub(super) fn coordinates(
    mapping: &material::Mapping,
    base: &str,
    notes: &mut Vec<String>,
) -> String {
    let mut parts = mapping.transform.map(|value| format!("{value:.9}"));
    for function in &mapping.functions {
        let slots: &[usize] = match function.kind {
            2 => &[0, 1],
            3 => &[0],
            4 => &[1],
            5 => &[2],
            6 => &[3],
            other => {
                notes.push(format!(
                    "{} animation kind {other} has no coordinate binding",
                    mapping.role
                ));
                continue;
            }
        };
        let value = match expression(function) {
            Ok(value) => value,
            Err(error) => {
                notes.push(format!(
                    "{} animation keeps its stored transform: {error:#}",
                    mapping.role
                ));
                continue;
            }
        };
        for &slot in slots {
            parts[slot] = format!("({value}).x");
        }
    }
    format!(
        "mad({base},float2({},{}),float2({},{}))",
        parts[0], parts[1], parts[2], parts[3]
    )
}

pub(super) fn expression(function: &material::Function) -> Result<String> {
    let formula = Property::mapping(&function.data)?;
    ensure!(
        function.data[2] == 0 || function.kind == 1,
        "Scalar material animation requires a scalar mapping"
    );
    formula.shader(
        &|index| {
            match index {
                0 => {
                    ensure!(
                        function.input.is_empty(),
                        "Unbound material input {:?}",
                        function.input
                    );
                    ensure!(
                        function.period.is_finite() && function.period > 0.,
                        "Material clock has no positive period"
                    );
                    // MCC 002626E8 keeps periodic time unwrapped. Other curves use
                    // truncation toward zero rather than floor, including negative time.
                    let phase = format!("reachPhase(time,{:e})", function.period);
                    Ok(if function.data[0] == 3 {
                        phase
                    } else {
                        format!("({phase}-trunc({phase}))")
                    })
                }
                1 => {
                    ensure!(
                        function.range.is_empty(),
                        "Unbound material range {:?}",
                        function.range
                    );
                    Ok("0.0".into())
                }
                _ => anyhow::bail!("Unknown material mapping input {index}"),
            }
        },
        &|wave, time| Ok(format!("reachPeriodic({wave},({time}).x).xxxx")),
    )
}

/// Integer loads preserve source table quantization without sampler state or filtered row edges.
/// Negative phases retain MCC's truncation and signed interpolation fraction. Recover the byte
/// before applying the source reciprocal, since hardware UNORM division can round differently.
pub(super) fn periodic(
    native: &mut Reader,
    graph: &mut Graph,
    bindings: &mut Vec<(usize, String)>,
) -> Result<String> {
    let register = bindings.iter().map(|(slot, _)| slot + 1).max().unwrap_or(0);
    ensure!(
        register < MAX_TEXTURE_REGISTERS,
        "Material curve lookup exceeds texture register capacity"
    );
    let symbol = "reach-material-periodic";
    if !graph.nodes.iter().any(|node| node["symbol"] == symbol) {
        let rgba = Property::periodic_tables()
            .iter()
            .chain(Property::transition_tables())
            .flat_map(|&value| [value, value, value, 255])
            .collect();
        rgba_texture(native, graph, symbol, 1024, 20, rgba, false, Mips::None)?;
    }
    bindings.push((register, symbol.into()));
    Ok(format!(
        r#"
Texture2D<float4> ReachPeriodic:register(t{register});
int reachTrunc(float value) {{ return value>=-2147483648.0 && value<2147483648.0 ? (int)value : (-2147483647-1); }}
precise float reachPeriodic(uint wave,float phase) {{
    if(wave>=12) {{
        phase=saturate(phase);
        if(wave==12) return phase;
        precise float scaled=phase*1023;
        precise float fraction=scaled-trunc(scaled);
        precise float shifted=scaled-0.1;
        int index=(int)(shifted+(shifted>=0?0.5:-0.5));
        precise float a=round(ReachPeriodic.Load(int3(index,wave,0)).r*255)*(1.0/255.0);
        if(index==1023) return saturate(a);
        precise float b=round(ReachPeriodic.Load(int3(min(index+1,1023),wave,0)).r*255)*(1.0/255.0);
        precise float value=(1-fraction)*a+fraction*b;
        return saturate(value);
    }}
    if(wave<2) return 1.0-wave;
    precise float scaled=phase*36.57143020629883;
    precise float fraction=scaled-(float)reachTrunc(scaled);
    precise float whole=scaled-fraction;
    int cursor=reachTrunc(whole+(whole>=0?0.5:-0.5));
    uint index=(uint)cursor&1023;
    precise float a=round(ReachPeriodic.Load(int3(index,wave,0)).r*255)*(1.0/255.0);
    precise float b=round(ReachPeriodic.Load(int3((index+1)&1023,wave,0)).r*255)*(1.0/255.0);
    if((wave==6 || wave==7) && a>0.75 && b<0.25) b+=1;
    precise float value=(1-fraction)*a+fraction*b;
    if((wave==6 || wave==7) && value>1) value-=1;
    return value;
}}
"#
    ))
}
