use super::Options;
use crate::halo_reach::level::mission::{Node, Program, analysis, vocabulary};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Compiled {
    pub payload: Value,
    pub scripts: Vec<usize>,
    pub globals: Vec<usize>,
    pub omitted: Vec<usize>,
    pub sites: Vec<u32>,
}

struct Builder<'a> {
    source: &'a Program,
    native: Option<&'a super::NativeBindings>,
    scripts: BTreeMap<usize, usize>,
    globals: BTreeMap<usize, usize>,
    code: Vec<Value>,
    sites: Vec<u32>,
}

pub(super) fn compile(source: &Program, options: &Options) -> Result<Compiled> {
    let dependencies = analysis::read(source)?;
    let entries = entrypoints(source, options)?;
    let (scripts, globals) = selected(&dependencies, &entries)?;
    validate_initializers(source, &dependencies, &globals)?;
    let scripts: Vec<_> = scripts.into_iter().collect();
    let globals: Vec<_> = globals.into_iter().collect();
    let mut builder = Builder {
        source,
        native: options.native.as_ref(),
        scripts: scripts
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i + 1))
            .collect(),
        globals: globals
            .iter()
            .enumerate()
            .map(|(i, g)| (*g, i + 1))
            .collect(),
        code: Vec::new(),
        sites: Vec::new(),
    };
    let mut script_rows = Vec::new();
    for index in &scripts {
        let script = &source.scripts[*index];
        ensure!(
            matches!(script.kind, 0..=3),
            "Script {} requires an unsupported execution context",
            script.name
        );
        scalar_type(script.return_type)?;
        for parameter in &script.parameters {
            scalar_type(parameter.value_type)?;
        }
        let start = builder.code.len() + 1;
        builder
            .expression(
                script.root.context("Selected script has no root")?,
                script.parameters.len(),
                0,
            )
            .with_context(|| format!("Compiling mission script {}", script.name))?;
        builder.emit(json!(["cast", script.return_type]), 0);
        builder.emit(json!(["return"]), 0);
        script_rows.push(json!([
            index,
            script.name,
            script.kind,
            start,
            script
                .parameters
                .iter()
                .map(|p| p.value_type)
                .collect::<Vec<_>>(),
            script.return_type
        ]));
    }
    let mut global_rows = Vec::new();
    for index in &globals {
        let global = &source.globals[*index];
        scalar_type(global.value_type)?;
        let start = builder.code.len() + 1;
        builder
            .expression(
                global.root.context("Selected global has no initializer")?,
                0,
                0,
            )
            .with_context(|| format!("Compiling mission global {}", global.name))?;
        builder.emit(json!(["cast", global.value_type]), 0);
        builder.emit(json!(["return"]), 0);
        global_rows.push(json!([index, global.name, global.value_type, start]));
    }
    ensure!(
        builder.code.len() <= 16384,
        "Selected mission exceeds its instruction capacity"
    );
    let omitted = source
        .scripts
        .iter()
        .enumerate()
        .filter(|(i, s)| matches!(s.kind, 0 | 2) && !entries.contains(i))
        .map(|(i, _)| i)
        .collect();
    let payload = json!({
        "schema":1, "namespace":options.namespace, "source":source.source_metadata_sha256,
        "native":options.native.as_ref().map_or(json!(false), |v| json!(v)),
        "entries":entries.iter().map(|i| builder.scripts[i]).collect::<Vec<_>>(),
        "scripts":script_rows, "globals":global_rows, "code":builder.code,
    });
    Ok(Compiled {
        payload,
        scripts,
        globals,
        omitted,
        sites: builder.sites,
    })
}

fn entrypoints(source: &Program, options: &Options) -> Result<Vec<usize>> {
    let mut entries = Vec::new();
    for name in &options.entrypoints {
        let matches: Vec<_> = source
            .scripts
            .iter()
            .enumerate()
            .filter(|(_, s)| &s.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "Mission entry {name:?} must identify exactly one script"
        );
        let (index, script) = matches[0];
        ensure!(!entries.contains(&index), "Duplicate mission entry {name}");
        ensure!(
            script.parameters.is_empty() && matches!(script.kind, 0..=3),
            "Mission entry {name} needs arguments or an unsupported execution context"
        );
        entries.push(index);
    }
    Ok(entries)
}

fn selected(
    dependencies: &analysis::Analysis,
    entries: &[usize],
) -> Result<(BTreeSet<usize>, BTreeSet<usize>)> {
    let mut scripts: BTreeSet<_> = entries.iter().copied().collect();
    let mut globals = BTreeSet::new();
    loop {
        let before = (scripts.len(), globals.len());
        for dependency in scripts
            .iter()
            .map(|i| &dependencies.scripts[*i])
            .chain(
                globals
                    .iter()
                    .map(|i| &dependencies.global_initializers[*i]),
            )
            .collect::<Vec<_>>()
        {
            ensure!(
                dependency.external_globals.is_empty(),
                "Selected mission code reads an unbound external global: {:?}",
                dependency.external_globals
            );
            ensure!(
                dependency.unresolved_script_values.is_empty(),
                "Selected mission code has an unresolved script value"
            );
        }
        let selected = scripts
            .iter()
            .map(|i| &dependencies.scripts[*i])
            .chain(
                globals
                    .iter()
                    .map(|i| &dependencies.global_initializers[*i]),
            )
            .collect::<Vec<_>>();
        let calls: Vec<_> = selected
            .iter()
            .flat_map(|d| d.calls.iter().chain(&d.script_values))
            .copied()
            .collect();
        let used: Vec<_> = selected.iter().flat_map(|d| &d.globals).copied().collect();
        scripts.extend(calls);
        globals.extend(used);
        if before == (scripts.len(), globals.len()) {
            break;
        }
    }
    ensure!(
        scripts.len() <= 128 && globals.len() <= 192,
        "Selected mission exceeds the controller's script or global capacity"
    );
    Ok((scripts, globals))
}

fn validate_initializers(
    source: &Program,
    dependencies: &analysis::Analysis,
    globals: &BTreeSet<usize>,
) -> Result<()> {
    // Initializers run before scheduling. Reject latent operations even when they are hidden
    // in a static call or an untaken branch, rather than exporting a controller that fails later.
    let mut initializer_calls = BTreeSet::new();
    let mut pending = Vec::new();
    for index in globals {
        let dependency = &dependencies.global_initializers[*index];
        ensure!(
            !dependency
                .operations
                .iter()
                .any(|op| matches!(op, 22..=26 | 96 | 103 | 1090)),
            "Global {} contains a scheduling or native operation",
            source.globals[*index].name
        );
        pending.extend(dependency.calls.iter().copied());
    }
    while let Some(index) = pending.pop() {
        if !initializer_calls.insert(index) {
            continue;
        }
        let dependency = &dependencies.scripts[index];
        ensure!(
            !dependency
                .operations
                .iter()
                .any(|op| matches!(op, 22..=26 | 96 | 103 | 1090)),
            "Global initializer calls scheduling or native script {}",
            source.scripts[index].name
        );
        pending.extend(dependency.calls.iter().copied());
    }
    Ok(())
}

fn scalar_type(ty: u16) -> Result<()> {
    ensure!(
        matches!(ty, 4..=10),
        "Mission value type {ty} needs a target binding or unsupported conversion"
    );
    Ok(())
}

impl Builder<'_> {
    fn emit(&mut self, value: Value, source: u32) -> usize {
        self.sites.push(source);
        self.code.push(value);
        self.code.len()
    }

    fn jump(&mut self, at: usize, target: usize) {
        self.code[at - 1][1] = json!(target);
    }

    fn arguments(&self, node: &Node) -> Result<Vec<u32>> {
        let head = self.source.node(node.value)?;
        ensure!(
            head.kind() == 9 && head.value_type == 2 && head.opcode == node.opcode,
            "Mission call has an invalid function-name head"
        );
        let mut next = head.next;
        let mut args = Vec::new();
        while let Some(datum) = next {
            ensure!(args.len() < 256, "Mission call has too many arguments");
            args.push(datum);
            next = self.source.node(datum)?.next;
        }
        Ok(args)
    }

    fn expression(&mut self, datum: u32, parameters: usize, depth: usize) -> Result<()> {
        ensure!(depth < 256, "Mission expression nesting exceeds 256");
        ensure!(
            self.code.len() < 16384,
            "Selected mission exceeds its instruction capacity"
        );
        let node = self.source.node(datum)?;
        scalar_type(node.value_type)?;
        match node.kind() {
            9 => {
                let ty = node.literal_type();
                scalar_type(ty)?;
                match ty {
                    4 => {
                        self.emit(json!(["push", 0]), datum);
                    }
                    5 => {
                        self.emit(json!(["push", node.value as u8 != 0]), datum);
                    }
                    6 => {
                        let value = f32::from_bits(node.value);
                        ensure!(value.is_finite(), "Nonfinite mission real literal");
                        self.emit(json!(["push", value]), datum);
                    }
                    7 => {
                        self.emit(json!(["push", node.value as i16]), datum);
                    }
                    8 => {
                        self.emit(json!(["push", node.value as i32]), datum);
                    }
                    9 => {
                        let bytes = node
                            .text_bytes
                            .as_ref()
                            .context("Mission string has no retained bytes")?;
                        ensure!(
                            bytes.len() <= 4096,
                            "Mission string exceeds execution capacity"
                        );
                        self.emit(json!(["bytes", bytes]), datum);
                    }
                    10 => {
                        let index = usize::from(node.value as u16);
                        self.emit(
                            json!([
                                "push",
                                self.scripts.get(&index).context("Unbound script literal")?
                            ]),
                            datum,
                        );
                    }
                    _ => unreachable!(),
                }
            }
            13 => {
                let index = usize::from(node.value as u16);
                let global = self.globals.get(&index).context("Unbound mission global")?;
                self.emit(json!(["global", global]), datum);
            }
            29 => {
                ensure!(
                    (node.value as usize) < parameters,
                    "Mission parameter is outside its call frame"
                );
                self.emit(json!(["parameter", node.value + 1]), datum);
            }
            10 => {
                let index = usize::from(node.opcode);
                let script = &self.source.scripts[index];
                ensure!(
                    script.kind == 3,
                    "Direct calls require a static mission script"
                );
                let args = self.arguments(node)?;
                ensure!(
                    args.len() == script.parameters.len(),
                    "Call to {} has the wrong argument count",
                    script.name
                );
                for (arg, parameter) in args.iter().zip(&script.parameters) {
                    self.expression(*arg, parameters, depth + 1)?;
                    self.emit(json!(["cast", parameter.value_type]), datum);
                }
                self.emit(json!(["call", self.scripts[&index], args.len()]), datum);
            }
            8 => {
                let args = self.arguments(node)?;
                self.operation(node, &args, parameters, depth + 1)?;
            }
            kind => bail!("Mission expression kind {kind} is not executable"),
        }
        self.emit(json!(["cast", node.value_type]), datum);
        Ok(())
    }

    fn operation(
        &mut self,
        node: &Node,
        args: &[u32],
        parameters: usize,
        depth: usize,
    ) -> Result<()> {
        let datum = node.datum;
        let name = vocabulary::get()
            .functions
            .get(&node.opcode)
            .map_or("unknown", String::as_str);
        let arity = |min, max| -> Result<()> {
            ensure!(
                (min..=max).contains(&args.len()),
                "Mission operation {name} has unsupported argument count {}",
                args.len()
            );
            Ok(())
        };
        match node.opcode {
            0 => self.begin(node, args, parameters, depth)?,
            4 => self.branch(node, args, parameters, depth)?,
            6 => self.assign(node, args, parameters, depth)?,
            7 | 8 => self.short_circuit(node, args, parameters, depth)?,
            9..=21 | 37 | 38 => {
                match node.opcode {
                    13 | 16..=21 => arity(2, 2)?,
                    9..=12 => arity(2, 256)?,
                    37 => arity(1, 1)?,
                    38 => arity(3, 3)?,
                    _ => arity(1, 256)?,
                }
                for arg in args {
                    self.expression(*arg, parameters, depth)?;
                }
                self.emit(json!(["operator", node.opcode, args.len()]), datum);
            }
            22 => {
                arity(1, 1)?;
                self.expression(args[0], parameters, depth)?;
                self.emit(json!(["sleep"]), datum);
            }
            23 => {
                arity(0, 0)?;
                self.emit(json!(["forever"]), datum);
            }
            24 => {
                // A constant interval and timeout avoid guessing default or effectful optional
                // argument timing until the Reach consumer has been independently executed.
                arity(2, 3)?;
                let interval = self.integer(args[1])?;
                let timeout = args
                    .get(2)
                    .map(|d| self.integer(*d))
                    .transpose()?
                    .unwrap_or(-1);
                ensure!(
                    interval > 0 && timeout >= -1,
                    "Mission wait interval or timeout is invalid"
                );
                let condition = self.code.len() + 1;
                self.expression(args[0], parameters, depth)?;
                self.emit(json!(["cast", 5]), datum);
                self.emit(json!(["wait", condition, interval, timeout]), datum);
            }
            26 => {
                arity(1, 1)?;
                let target = self.source.node(args[0])?;
                ensure!(
                    target.kind() == 9 && target.literal_type() == 10,
                    "Mission wake requires a constant script reference"
                );
                let index = usize::from(target.value as u16);
                ensure!(
                    self.source.scripts[index].kind == 1,
                    "Mission wake currently requires a dormant target"
                );
                self.emit(json!(["wake", self.scripts[&index]]), datum);
            }
            39 => {
                arity(0, 0)?;
                self.emit(json!(["push", 0]), datum);
            }
            96 | 103 => {
                arity(1, 1)?;
                let native = self
                    .native
                    .context("Native object operation needs explicit target bindings")?;
                let slot = native.object(self.source, args[0])?;
                self.emit(json!(["object", slot, node.opcode == 96]), datum);
            }
            1090 => {
                arity(0, 0)?;
                ensure!(
                    self.native.is_some_and(|n| n.completion),
                    "Native mission completion needs an explicit target binding"
                );
                self.emit(json!(["complete"]), datum);
            }
            40..=42 => {
                arity(
                    if node.opcode == 41 { 2 } else { 1 },
                    if node.opcode == 41 { 2 } else { 1 },
                )?;
                for arg in args {
                    self.expression(*arg, parameters, depth)?;
                    self.emit(json!(["drop"]), datum);
                }
                // Assembly marks these retail MCC operations as null. Their argument
                // expressions still execute, and no native gameplay effect is invented.
                self.emit(json!(["push", 0]), datum);
            }
            _ => bail!(
                "Unsupported mission operation {name} ({:#06X}) at expression {datum:08X}",
                node.opcode
            ),
        }
        Ok(())
    }

    fn arity(&self, node: &Node, args: &[u32], min: usize, max: usize) -> Result<()> {
        let name = vocabulary::get()
            .functions
            .get(&node.opcode)
            .map_or("unknown", String::as_str);
        ensure!(
            (min..=max).contains(&args.len()),
            "Mission operation {name} has unsupported argument count {}",
            args.len()
        );
        Ok(())
    }

    fn begin(&mut self, node: &Node, args: &[u32], parameters: usize, depth: usize) -> Result<()> {
        let datum = node.datum;

        if args.is_empty() {
            self.emit(json!(["push", 0]), datum);
        }
        for (i, arg) in args.iter().enumerate() {
            self.expression(*arg, parameters, depth)?;
            if i + 1 != args.len() {
                self.emit(json!(["drop"]), datum);
            }
        }

        Ok(())
    }

    fn branch(&mut self, node: &Node, args: &[u32], parameters: usize, depth: usize) -> Result<()> {
        let datum = node.datum;

        self.arity(node, args, 2, 3)?;
        self.expression(args[0], parameters, depth)?;
        self.emit(json!(["cast", 5]), datum);
        let branch = self.emit(json!(["false", 0]), datum);
        self.expression(args[1], parameters, depth)?;
        let end = self.emit(json!(["jump", 0]), datum);
        self.jump(branch, self.code.len() + 1);
        if args.len() == 3 {
            self.expression(args[2], parameters, depth)?;
        } else {
            self.emit(json!(["push", 0]), datum);
        }
        self.jump(end, self.code.len() + 1);

        Ok(())
    }

    fn assign(&mut self, node: &Node, args: &[u32], parameters: usize, depth: usize) -> Result<()> {
        let datum = node.datum;

        self.arity(node, args, 2, 2)?;
        let target = self.source.node(args[0])?;
        ensure!(
            target.kind() == 13 && target.value as u16 & 0x8000 == 0,
            "Mission set requires a local global"
        );
        let index = usize::from(target.value as u16);
        self.expression(args[1], parameters, depth)?;
        self.emit(
            json!(["cast", self.source.globals[index].value_type]),
            datum,
        );
        self.emit(json!(["set", self.globals[&index]]), datum);

        Ok(())
    }

    fn short_circuit(
        &mut self,
        node: &Node,
        args: &[u32],
        parameters: usize,
        depth: usize,
    ) -> Result<()> {
        let datum = node.datum;

        self.arity(node, args, 1, 256)?;
        let mut exits = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            self.expression(*arg, parameters, depth)?;
            self.emit(json!(["cast", 5]), datum);
            if i + 1 != args.len() {
                exits.push(self.emit(
                    json!([if node.opcode == 7 { "false" } else { "true" }, 0]),
                    datum,
                ));
            }
        }
        let end = self.emit(json!(["jump", 0]), datum);
        let shortcut = self.code.len() + 1;
        self.emit(json!(["push", node.opcode == 8]), datum);
        for at in exits {
            self.jump(at, shortcut);
        }
        self.jump(end, self.code.len() + 1);

        Ok(())
    }
    fn integer(&self, datum: u32) -> Result<i32> {
        let node = self.source.node(datum)?;
        ensure!(
            node.kind() == 9,
            "Mission wait optional arguments must be constant integers"
        );
        match node.literal_type() {
            7 => Ok(i32::from(node.value as i16)),
            8 => Ok(node.value as i32),
            _ => bail!("Mission wait optional arguments must be constant integers"),
        }
    }
}

/// ASCII-only Lua literals preserve JSON strings without exposing source text as code.
pub(super) fn lua(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Null => bail!("Null is not an executable Lua value"),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => {
            let mut result = String::from("\"");
            for byte in v.bytes() {
                use std::fmt::Write;
                write!(result, "\\{byte:03}")?;
            }
            result.push('"');
            result
        }
        Value::Array(v) => format!(
            "{{{}}}",
            v.iter().map(lua).collect::<Result<Vec<_>>>()?.join(",")
        ),
        Value::Object(v) => format!(
            "{{{}}}",
            v.iter()
                .map(|(k, v)| Ok(format!("[{}]={}", lua(&json!(k))?, lua(v)?)))
                .collect::<Result<Vec<_>>>()?
                .join(",")
        ),
    })
}
