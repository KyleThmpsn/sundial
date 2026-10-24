use super::{instruction::Emitter, *};

impl Program {
    pub fn resources(&self) -> Result<BTreeMap<(u32, u32), (u32, u32)>> {
        let options = Options::default();
        let mut emitter = Emitter {
            program: self,
            options: &options,
            declarations: BTreeMap::new(),
            handles: BTreeMap::new(),
            resources: BTreeMap::new(),
        };
        for block in &self.order {
            for instruction in &self.blocks[block] {
                if instruction.code.contains("@dx.op.createHandleFromBinding(")
                    || instruction.code.contains("@dx.op.annotateHandle(")
                {
                    emitter.expression(instruction.id, &instruction.code)?;
                }
            }
        }
        Ok(emitter.resources)
    }

    pub fn hlsl(&self, options: &Options) -> Result<String> {
        ensure!(
            options
                .targets
                .iter()
                .all(|target| self.outputs.iter().any(|f| f.index == *target)),
            "Requested DXIL target is absent"
        );
        let mut emitter = Emitter {
            program: self,
            options,
            declarations: BTreeMap::new(),
            handles: BTreeMap::new(),
            resources: BTreeMap::new(),
        };
        let mut statements = BTreeMap::new();
        for block in &self.order {
            let mut rows = Vec::new();
            for instruction in &self.blocks[block] {
                let (typ, text) = emitter
                    .expression(instruction.id, &instruction.code)
                    .with_context(|| format!("DXIL block {block}: {}", instruction.code))?;
                if typ == Some("void") {
                    rows.push(text.context("Missing DXIL side effect")?);
                } else if let Some(typ) = typ {
                    let id = instruction.id.context("Missing DXIL instruction result")?;
                    emitter.declarations.insert(id, typ);
                    if let Some(text) = text {
                        rows.push(format!("v{id} = {text};"));
                    }
                }
            }
            statements.insert(*block, rows);
        }
        let mut result = Vec::new();
        for (&(class, slot), &(kind, size)) in &emitter.resources {
            result.push(match class {
                0 => format!(
                    "{}<{}4> t{slot}:register(t{});",
                    match options
                        .samples
                        .get(&slot)
                        .map_or(kind, |sample| sample.target_kind)
                    {
                        2 => "Texture2D",
                        4 => "Texture3D",
                        5 => "TextureCube",
                        _ => bail!("Unsupported DXIL texture kind"),
                    },
                    if size & 0xFF == 5 { "uint" } else { "float" },
                    options.texture_registers.get(&slot).unwrap_or(&slot)
                ),
                2 => {
                    ensure!(
                        size > 0 && size <= 65536,
                        "DXIL constant-buffer size differs"
                    );
                    format!(
                        "cbuffer C{slot}:register(b{slot}) {{ float4 cb{slot}[{}]; }};",
                        size.div_ceil(16)
                    )
                }
                3 => format!("SamplerState s{slot}:register(s{slot});"),
                _ => bail!("Unsupported DXIL resource declaration"),
            });
        }
        for (&slot, &rows) in &options.buffers {
            ensure!(rows > 0 && rows <= 4096, "Renderer buffer extent differs");
            ensure!(
                !emitter.resources.contains_key(&(2, slot)),
                "Renderer buffer conflicts with a source buffer"
            );
            result.push(format!(
                "cbuffer C{slot}:register(b{slot}) {{ float4 cb{slot}[{rows}]; }};"
            ));
        }
        let mut params = self
            .inputs
            .iter()
            .enumerate()
            .map(|(i, field)| {
                format!(
                    "{}{} i{i}:{}{}",
                    if field.integer { "uint" } else { "float" },
                    if field.width > 1 {
                        field.width.to_string()
                    } else {
                        String::new()
                    },
                    field.semantic,
                    field.index
                )
            })
            .collect::<Vec<_>>();
        for (i, field) in self.outputs.iter().enumerate() {
            if options.targets.contains(&field.index) {
                params.push(format!("out float4 o{i}:SV_Target{}", field.index));
            }
        }
        result.push(format!("void main({}) {{", params.join(",")));
        result.extend(
            emitter
                .declarations
                .iter()
                .map(|(id, typ)| format!("{typ} v{id};")),
        );
        for (i, field) in self.outputs.iter().enumerate() {
            if options.targets.contains(&field.index) {
                result.push(format!("o{i}=0;"));
            }
        }
        let mut visited = BTreeSet::new();
        self.region(
            0,
            None,
            &statements,
            &emitter.declarations,
            &mut visited,
            &mut result,
        )?;
        ensure!(
            visited.len() == self.blocks.len(),
            "DXIL control flow could not be structured completely"
        );
        result.push("}".into());
        Ok(result.join("\n") + "\n")
    }

    fn edge(
        &self,
        origin: u32,
        dest: u32,
        types: &BTreeMap<u32, &'static str>,
        result: &mut Vec<String>,
    ) -> Result<()> {
        let mut copies = Vec::new();
        let predecessors = self
            .edges
            .iter()
            .filter(|(_, targets)| targets.contains(&dest))
            .map(|(block, _)| *block)
            .collect::<BTreeSet<_>>();
        for instruction in &self.blocks[&dest] {
            if let Some(phi) = instruction.code.strip_prefix("phi ") {
                let (_, pairs) = phi.split_once(' ').context("Malformed DXIL phi")?;
                let mut incoming = BTreeMap::new();
                for pair in arguments(pairs)? {
                    let pair = pair
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .context("DXIL phi pair")?;
                    let parts = arguments(pair)?;
                    ensure!(
                        parts.len() == 2 && incoming.insert(id(parts[1])?, parts[0]).is_none(),
                        "Ambiguous DXIL phi predecessor"
                    );
                }
                ensure!(
                    incoming.keys().copied().collect::<BTreeSet<_>>() == predecessors,
                    "Incomplete DXIL phi predecessors"
                );
                let id = instruction.id.context("DXIL phi result")?;
                let rhs = value(incoming[&origin])?;
                result.push(format!("{} edge_{origin}_{dest}_{id} = {rhs};", types[&id]));
                copies.push(format!("v{id} = edge_{origin}_{dest}_{id};"));
            }
        }
        // Phi assignments are simultaneous, including when operands reference
        // another phi. Capture every incoming value before writing any result.
        result.extend(copies);
        Ok(())
    }

    fn region(
        &self,
        mut block: u32,
        stop: Option<u32>,
        statements: &BTreeMap<u32, Vec<String>>,
        types: &BTreeMap<u32, &'static str>,
        visited: &mut BTreeSet<u32>,
        result: &mut Vec<String>,
    ) -> Result<()> {
        while Some(block) != stop {
            ensure!(
                visited.insert(block),
                "DXIL control flow is not a supported structured region"
            );
            result.extend(statements[&block].iter().cloned());
            match self.edges[&block].as_slice() {
                [] => break,
                &[dest] => {
                    self.edge(block, dest, types, result)?;
                    block = dest;
                }
                &[yes, no] => {
                    let mut post = self.post[&block].clone();
                    post.remove(&block);
                    let merge = post
                        .iter()
                        .find(|candidate| post.is_subset(&self.post[*candidate]))
                        .copied();
                    let branch = &self.blocks[&block].last().context("DXIL branch")?.code;
                    let condition =
                        value(branch.split(',').next().context("DXIL branch condition")?)?;
                    result.push(format!("if ({condition}) {{"));
                    self.edge(block, yes, types, result)?;
                    self.region(yes, merge, statements, types, visited, result)?;
                    result.push("} else {".into());
                    self.edge(block, no, types, result)?;
                    self.region(no, merge, statements, types, visited, result)?;
                    result.push("}".into());
                    if let Some(merge) = merge {
                        block = merge;
                    } else {
                        break;
                    }
                }
                _ => bail!("DXIL branch has unsupported successors"),
            }
            ensure!(
                result.len() <= 262144,
                "Expanded DXIL program exceeds budget"
            );
        }
        Ok(())
    }
}
