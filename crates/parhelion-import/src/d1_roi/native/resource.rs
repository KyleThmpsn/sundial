use super::*;
use crate::tiger::shader::{
    identity::Stage,
    program::{Program, Template},
};

pub(super) struct Shader {
    pub stage: Stage,
    pub deferred: bool,
    pub layout: u16,
    pub depth: Option<super::super::expression::Depth>,
    /// The texture register of the reticle mask a pixel program loads.
    pub reticle_mask: Option<u32>,
}

impl Shader {
    /// Stored layouts without a per-vertex detail scale that a source vertex
    /// program may still name: weighted 9 and unweighted 123.
    fn no_detail_scale(&self) -> bool {
        self.stage == Stage::Vertex && matches!(self.layout, 9 | 123)
    }

    fn symbol(&self, tag: u32) -> String {
        let mut symbol = format!(
            "shader-{tag:08X}-{}",
            if self.deferred { "deferred" } else { "source" }
        );
        if self.no_detail_scale() {
            symbol.push_str(&format!("-layout-{}", self.layout));
        }
        if let Some(depth) = self.depth {
            symbol.push_str(&format!("-depth-{}-{}", depth.source, depth.native));
        }
        if let Some(slot) = self.reticle_mask {
            symbol.push_str(&format!("-reticle-{slot}"));
        }
        symbol
    }
}

impl Assembly<'_> {
    pub(super) fn texture(&mut self, tag: u32) -> Result<String> {
        let symbol = format!("texture-{tag:08X}");
        if self.symbols.contains(&symbol) {
            return Ok(symbol);
        }
        let texture = self
            .source
            .linear_texture(tag, self.options.xg_library.as_deref())?;
        self.texture_bytes(&symbol, texture)
    }

    pub(super) fn texture_bytes(
        &mut self,
        symbol: &str,
        texture: super::super::texture::Texture,
    ) -> Result<String> {
        let subtype = if texture.layers == 6 { 2 } else { 1 };
        let template = self.template(32, subtype)?;
        let data_template = self.template(40, subtype)?;
        let mut header = vec![0; 40];
        put(
            &mut header,
            0,
            &u32::try_from(texture.bytes.len())?.to_le_bytes(),
        )?;
        put(&mut header, 4, &texture.format.to_le_bytes())?;
        put(&mut header, 12, &0xCAFEu16.to_le_bytes())?;
        for (at, value) in [
            (14, texture.width),
            (16, texture.height),
            (18, 1),
            (20, texture.layers),
        ] {
            put(&mut header, at, &value.to_le_bytes())?;
        }
        header[23] = texture.mips;
        header[25] = 1;
        header[31] = 1;
        header[32] = if texture.layers == 6 { 2 } else { 0 };
        header[33] = 3;
        header[34] = 1;
        put(&mut header, 36, &u32::MAX.to_le_bytes())?;
        crate::tiger::texture::resident(&mut header, texture.bytes.len())?;
        let data = format!("{symbol}-data");
        self.node(&data, data_template, &texture.bytes, Some(symbol), &[])?;
        self.node(symbol, template, &header, Some(&data), &[])
    }

    pub(super) fn sampler(&mut self, tag: u32) -> Result<String> {
        let symbol = format!("sampler-{tag:08X}");
        if self.symbols.contains(&symbol) {
            return Ok(symbol);
        }
        let header = self.source.read(tag)?;
        let descriptor = self.source.read(self.source.reference(tag)?)?;
        ensure!(
            header.0 == [0; 8] && descriptor.0.len() == 60 && descriptor.0[52..] == [255; 8],
            "Unknown source sampler storage"
        );
        let template = self.template(34, 1)?;
        let native_header = self.native.tag(template, None)?;
        let data_template = self.native.reference(template)?;
        let native_descriptor = self.native.tag(data_template, None)?;
        ensure!(
            native_descriptor.0.len() == 52,
            "Native sampler descriptor extent differs"
        );
        let data = format!("{symbol}-data");
        self.node(
            &data,
            data_template,
            &descriptor.0[..52],
            Some(&symbol),
            &[],
        )?;
        self.node(&symbol, template, &native_header.0, Some(&data), &[])
    }

    pub(super) fn shader(&mut self, tag: u32, inputs: Shader) -> Result<String> {
        let symbol = inputs.symbol(tag);
        let no_detail_scale = inputs.no_detail_scale();
        let Shader {
            stage,
            deferred,
            layout,
            depth,
            reticle_mask,
        } = inputs;
        if self.symbols.contains(&symbol) {
            return Ok(symbol);
        }
        let source = self.source.read(tag)?;
        let range = source.array_range(8, 1, Some(0x80800009))?;
        let raw = source.0.get(range).context("Source GPU program")?;
        let mut code = if stage == Stage::Vertex {
            super::super::shader::skinning(raw)
                .with_context(|| format!("Source vertex program {tag:08X}"))?
        } else if deferred {
            super::super::shader::deferred(raw, &self.glow)
                .with_context(|| format!("Source deferred program {tag:08X}"))?
        } else {
            super::super::shader::compact(raw)
                .with_context(|| format!("Source GPU program {tag:08X}"))?
        };
        if no_detail_scale {
            let specialized = super::super::shader::without_detail_scale(&code, layout)
                .with_context(|| format!("Source vertex program {tag:08X}, layout {layout}"))?;
            if specialized.approximated {
                // The absent input has no established source value. The import
                // reports the adopted base-UV convention instead of a guess.
                self.evidence
                    .push(json!({"shader":tag,"symbol":symbol,"layout":layout,
                    "approximation":"missing_detail_scale_uses_base_uv"}));
                self.approximations.insert(format!(
                    "Vertex program {tag:08X} reads a per-vertex detail scale that its layout {layout} draws do not store. Those draws use the base texture coordinates for the detail pair, following the source convention for that layout. Their detail texture scale is approximated."
                ));
            }
            code = specialized.code;
        }
        if let Some(depth) = depth {
            code = super::super::shader::depth(&code, &depth)
                .with_context(|| format!("Source transparent depth program {tag:08X}"))?;
        }
        if let Some(slot) = reticle_mask {
            code = super::super::shader::reticle_mask(&code, slot)
                .with_context(|| format!("Source sight program {tag:08X}"))?;
        }
        let template_tag = self.template(
            33,
            if matches!(stage, Stage::Hull | Stage::Domain) {
                1
            } else {
                stage.subtype()
            },
        )?;
        let template = Template::read(&mut self.native, template_tag)?;
        let program = Program::new(stage, code)?;
        let data = format!("{symbol}-bytecode");
        if matches!(stage, Stage::Hull | Stage::Domain) {
            self.node(
                &data,
                template.bytecode_tag(),
                program.bytecode(),
                Some(&symbol),
                &[],
            )?;
            self.node(
                &symbol,
                template.header_tag(),
                &program.header().0,
                Some(&data),
                &[],
            )?;
            let storage = if stage == Stage::Hull {
                "gpu_hull"
            } else {
                "gpu_domain"
            };
            for node in self.nodes.iter_mut().rev().take(2) {
                node["storage"] = json!(storage);
            }
            Ok(symbol)
        } else {
            let assets = program.assets(&symbol, &template)?;
            self.node(
                &data,
                assets.bytecode_template,
                &assets.bytecode,
                Some(&symbol),
                &[],
            )?;
            self.node(
                &symbol,
                assets.header_template,
                &assets.header,
                Some(&data),
                &[],
            )
        }
    }
}
