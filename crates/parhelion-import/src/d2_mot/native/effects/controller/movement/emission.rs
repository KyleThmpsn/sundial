//! Emit validated movement trees in native record order, then resolve their pointers.
use super::*;

pub(super) struct Emitter<'a> {
    pub s: &'a Source<'a>,
    pub tree: &'a Tree,
    pub template: &'a Payload,
    pub resources: &'a Resources,
    pub owner_tag: u32,
    pub template_records: &'a [Record],
}

impl Emitter<'_> {
    pub(super) fn instance_roots(&self, w: &mut Writer) -> Result<usize> {
        let s = self.s;
        let tree = self.tree;
        let template = self.template;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // ---- instance region ----
        let root_i_at = w.align(0x80803B73);
        let mut root_i = vec![0; ROOT_INSTANCE_SIZE];
        root_i[..0x30].copy_from_slice(&p.0[tree.root_i.at..tree.root_i.at + 0x30]);
        put(&mut root_i, 0, &owner_tag.to_le_bytes())?;
        put(&mut root_i, 4, &0x8080388Fu32.to_le_bytes())?;
        put(&mut root_i, 0x30, &[0; 0x20])?;
        w.record(tree.root_i.at, root_i);
        w.fixups.push(Fixup::Twin {
            at: root_i_at,
            twin: tree.root_d.at,
        });
        w.fixups.push(Fixup::Array {
            at: root_i_at + 0x30,
            count: tree.slots.len(),
            header: tree.slots_headers.inst,
        });
        w.fixups.push(Fixup::Array {
            at: root_i_at + 0x40,
            count: tree.scalars.len(),
            header: tree.scalars_headers.inst,
        });
        // settings1 instance
        let s1_i = s.record(tree.s1_d.twin)?;
        let at = w.record(
            s1_i.at,
            verbatim(p, s1_i, SETTINGS1_INSTANCE_SIZE, owner_tag)?,
        );
        w.fixups.push(Fixup::Twin {
            at,
            twin: tree.s1_d.at,
        });
        parent(w, p, s1_i, at)?;
        w.fixups.push(Fixup::Array {
            at: at + 0x20,
            count: tree.blocks.len(),
            header: tree.blocks_headers.inst,
        });
        // settings2 instance
        let s2_i = s.record(tree.s2_d.twin)?;
        let mut bytes = template.0
            [s2_i_template(template)?..s2_i_template(template)? + SETTINGS2_INSTANCE_SIZE]
            .to_vec();
        put(&mut bytes, 0, &owner_tag.to_le_bytes())?;
        for &(native, modern) in SETTINGS2_INSTANCE {
            put(&mut bytes, native, &p.bytes::<4>(s2_i.at + modern)?)?;
        }
        let at = w.record(s2_i.at, bytes);
        w.fixups.push(Fixup::Twin {
            at,
            twin: tree.s2_d.at,
        });
        parent(w, p, s2_i, at)?;
        w.fixups.push(Fixup::Array {
            at: at + 0x128,
            count: tree.states.len(),
            header: tree.states_headers.inst,
        });
        Ok(root_i_at)
    }

    pub(super) fn instance_slots(&self, w: &mut Writer) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // slots
        w.array_header(tree.slots_headers.inst, 0x808037CC, tree.slots.len());
        for slot in &tree.slots {
            let inst = s.record(slot.def.twin)?;
            let at = w.record(inst.at, verbatim(p, inst, SLOT_INSTANCE_SIZE, owner_tag)?);
            w.fixups.push(Fixup::Twin {
                at,
                twin: slot.def.at,
            });
            parent(w, p, inst, at)?;
            w.fixups.push(Fixup::Array {
                at: at + 0x20,
                count: slot.curves.len(),
                header: slot.curves_headers.inst,
            });
        }
        for slot in &tree.slots {
            w.array_header(slot.curves_headers.inst, 0x808037CF, slot.curves.len());
            for curve in &slot.curves {
                let hi = s.record(curve.header.twin)?;
                let at = w.record(hi.at, verbatim(p, hi, CURVE_INSTANCE_SIZE, owner_tag)?);
                w.fixups.push(Fixup::Twin {
                    at,
                    twin: curve.header.at,
                });
                parent(w, p, hi, at)?;
                let di = s.record(curve.data.twin)?;
                let at = w.record(di.at, verbatim(p, di, CURVE_INSTANCE_SIZE, owner_tag)?);
                w.fixups.push(Fixup::Twin {
                    at,
                    twin: curve.data.at,
                });
                parent(w, p, di, at)?;
                expression_instance(w, s, &curve.expression, owner_tag)?;
            }
            for curve in &slot.curves {
                input_instances(w, s, &curve.expression, owner_tag)?;
            }
        }
        Ok(())
    }

    pub(super) fn instance_expressions(&self, w: &mut Writer) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // scalar expressions
        w.array_header(tree.scalars_headers.inst, 0x808089F7, tree.scalars.len());
        for expression in &tree.scalars {
            expression_instance(w, s, expression, owner_tag)?;
        }
        for expression in &tree.scalars {
            input_instances(w, s, expression, owner_tag)?;
        }
        // blocks
        w.array_header(tree.blocks_headers.inst, 0x8080851B, tree.blocks.len());
        for block in &tree.blocks {
            let hi = s.record(block.holder.twin)?;
            let at = w.record(hi.at, verbatim(p, hi, HOLDER_INSTANCE_SIZE, owner_tag)?);
            w.fixups.push(Fixup::Twin {
                at,
                twin: block.holder.at,
            });
            parent(w, p, hi, at)?;
            expression_instance(w, s, &block.expression, owner_tag)?;
        }
        for block in &tree.blocks {
            input_instances(w, s, &block.expression, owner_tag)?;
        }
        Ok(())
    }

    pub(super) fn instance_states(
        &self,
        w: &mut Writer,
        template_state_instance: &[u8],
    ) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // state
        w.array_header(tree.states_headers.inst, 0x808037BA, tree.states.len());
        for state in &tree.states {
            let inst = s.record(state.twin)?;
            let modern_size = inst_extent(s, inst);
            // Extents include the marker line before the definition root that follows the state. A
            // state definition naming the child class at +14 has the child object behind its instance.
            let has_child = p.u32(state.at + 0x14)? == 0x808029B8;
            // The last row's extent also covers the marker line of the array that follows.
            let size = match (modern_size, has_child) {
                (0x360 | 0x370, false) => 0x210,
                (0x4A0 | 0x4B0, true) => 0x320,
                (other, _) => bail!("unsupported movement state instance size {other:X}"),
            };
            ensure!(
                p.u64(inst.at + 0x60)? == 0,
                "movement state instance names a child array"
            );
            let mut bytes = mapped(
                p,
                inst,
                size,
                STATE_INSTANCE,
                owner_tag,
                Some(template_state_instance),
            )?;
            if size == 0x320 {
                // The child object: a marker line naming its class, then its body, which the state
                // instance points at from +50.
                put(&mut bytes, 0x210, &[0; 12])?;
                put(&mut bytes, 0x21C, &0x808037BDu32.to_le_bytes())?;
                put(&mut bytes, 0x50, &(0x220i64 - 0x50).to_le_bytes())?;
            } else {
                put(&mut bytes, 0x50, &0u64.to_le_bytes())?;
            }
            let at = w.record(inst.at, bytes);
            w.fixups.push(Fixup::Twin { at, twin: state.at });
            parent(w, p, inst, at)?;
        }

        Ok(())
    }

    pub(super) fn definition_root(
        &self,
        w: &mut Writer,
        siblings: &mut Vec<(usize, u32)>,
    ) -> Result<usize> {
        let s = self.s;
        let tree = self.tree;
        let template = self.template;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        let td = template.pointer(24)?;
        // ---- definition region ----
        let root_d_at = w.align(0x8080388F);
        let mut bytes = mapped(
            p,
            tree.root_d,
            ROOT_DEFINITION_SIZE,
            ROOT_DEFINITION,
            owner_tag,
            Some(&template.0[td..td + ROOT_DEFINITION_SIZE]),
        )?;
        // +C8 holds one of two constants in both layouts.
        let c8: u32 = match p.u32(tree.root_d.at + 0x170)? {
            0 => 0,
            0x6D4 => 0x50C,
            other => bail!("movement root carries an unknown +170 value {other:X}"),
        };
        put(&mut bytes, 0xC8, &c8.to_le_bytes())?;
        // The native flag word at +48 has one more flag above the modern bit 16: the low 17 bits
        // stay and the rest move up a bit (80 of the 93 twins, the others retuned).
        let flags = p.u32(tree.root_d.at + 0x48)?;
        put(
            &mut bytes,
            0x48,
            &((flags & 0x1_FFFF) | ((flags & !0x1_FFFF) << 1)).to_le_bytes(),
        )?;
        // Three head references, 16 bytes modern and 8 native.
        for (native, modern) in [(0x108, 0x1B8), (0x118, 0x1D0), (0x180, 0x240)] {
            let (tag, unresolved) = head_reference(p, tree.root_d.at + modern, resources)?;
            put(&mut bytes, native, &tag.to_le_bytes())?;
            put(&mut bytes, native + 4, &[0; 4])?;
            if unresolved {
                siblings.push((root_d_at + native, tag));
            }
        }
        // The modern root replaced the scalar at +7C with an inline expression; a plain constant
        // (push constant 0, output) carries the scalar in its first lane.
        let inline = tree.root_d.at + 0x88;
        let (_, code, _) = s
            .raw_array(inline + 0x10, CODE, 1)?
            .context("modern movement root expression has no program")?;
        ensure!(
            code == [0x42, 0, 0x4C, 0],
            "movement root expression is not a plain constant"
        );
        let (_, constants, _) = s
            .raw_array(inline + 0x20, VECTORS, 16)?
            .context("modern movement root expression has no constant")?;
        put(&mut bytes, 0x7C, &constants[..4])?;
        w.record(tree.root_d.at, bytes);
        w.fixups.push(Fixup::Twin {
            at: root_d_at,
            twin: tree.root_i.at,
        });
        Ok(root_d_at)
    }

    pub(super) fn definition_settings(
        &self,
        w: &mut Writer,
        siblings: &mut Vec<(usize, u32)>,
    ) -> Result<usize> {
        let s = self.s;
        let tree = self.tree;
        let template = self.template;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        let s2_i = s.record(tree.s2_d.twin)?;
        let s1_i = s.record(tree.s1_d.twin)?;
        let td = template.pointer(24)?;
        // settings1 definition
        let at = w.record(
            tree.s1_d.at,
            verbatim(p, tree.s1_d, SETTINGS1_DEFINITION_SIZE, owner_tag)?,
        );
        w.fixups.push(Fixup::Twin { at, twin: s1_i.at });
        w.fixups.push(Fixup::Array {
            at: at + 0x10,
            count: tree.blocks.len(),
            header: tree.blocks_headers.def,
        });
        // settings2 definition: template tables, source head. After the modern settings come the
        // dropped inline expression's arrays and, on some projectiles, a trailer record: four
        // provider values, an expression valued block or a referenced provider. The native layout
        // keeps it as an embedded record right after its settings definition.
        let next_header = [
            tree.blocks_headers.def,
            tree.slots_headers.def,
            tree.scalars_headers.def,
            tree.states_headers.def,
        ]
        .into_iter()
        .filter(|header| *header != 0)
        .min()
        .context("movement owner names no definition array")?;
        let mut trailer = None;
        let mut at_word = tree.s2_d.at + MODERN_SETTINGS2_TRAILER;
        while at_word + 4 <= next_header && trailer.is_none() {
            let class = p.u32(at_word)?;
            if matches!(class, 0x80802A08 | 0x808029D5 | 0x808029D7) {
                trailer = Some((class, at_word + 4));
            }
            at_word += 4;
        }
        let mut bytes = template.0[td + ROOT_DEFINITION_SIZE + SETTINGS1_DEFINITION_SIZE
            ..td + ROOT_DEFINITION_SIZE + SETTINGS1_DEFINITION_SIZE + SETTINGS2_DEFINITION_SIZE]
            .to_vec();
        ensure!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) == 0x80809BD8,
            "native movement template lacks its settings2 definition"
        );
        put(&mut bytes, 0, &owner_tag.to_le_bytes())?;
        for &(native, modern) in SETTINGS2_DEFINITION {
            put(&mut bytes, native, &p.bytes::<4>(tree.s2_d.at + modern)?)?;
        }
        // Four head references, 16 bytes modern and 8 native.
        for (native, modern) in [(0x28, 0x28), (0x38, 0x40), (0x48, 0x58), (0x58, 0x70)] {
            let (tag, unresolved) = head_reference(p, tree.s2_d.at + modern, resources)?;
            put(&mut bytes, native, &tag.to_le_bytes())?;
            put(&mut bytes, native + 4, &[0; 4])?;
            if unresolved {
                siblings.push((w.out.len() + native, tag));
            }
        }
        if trailer.is_some() {
            bytes.truncate(SETTINGS2_PROVIDER);
        }
        let at = w.record(tree.s2_d.at, bytes);
        let mut trailer_body = None;
        if let Some((class, body)) = trailer {
            let base = w.out.len();
            let (embedded, offset) = Rewriter::new(p, resources, 0).embedded(class, body, base)?;
            w.out.extend(embedded);
            trailer_body = Some((body, base + offset));
        }
        // The settings name one expression valued block at +68 (modern +88), when there is one:
        // an input's, or their own trailer's.
        if p.u64(tree.s2_d.at + 0x88)? != 0 {
            let target = p.pointer(tree.s2_d.at + 0x88)?;
            match trailer_body {
                Some((modern, native)) if modern == target => put(
                    &mut w.out,
                    at + 0x68,
                    &(native as i64 - (at + 0x68) as i64).to_le_bytes(),
                )?,
                _ => w.fixups.push(Fixup::Into {
                    at: at + 0x68,
                    target,
                }),
            }
        } else {
            put(&mut w.out, at + 0x68, &0u64.to_le_bytes())?;
        }
        w.fixups.push(Fixup::Twin { at, twin: s2_i.at });
        w.fixups.push(Fixup::Array {
            at: at + 0x70,
            count: tree.slots.len(),
            header: tree.slots_headers.def,
        });
        w.fixups.push(Fixup::Array {
            at: at + 0x80,
            count: tree.scalars.len(),
            header: tree.scalars_headers.def,
        });
        w.fixups.push(Fixup::Array {
            at: at + 0x410,
            count: tree.states.len(),
            header: tree.states_headers.def,
        });
        for row in 0..SETTINGS2_ROWS {
            let (start, stride, hash) = SETTINGS2_NATIVE_ROWS;
            let native_row = at + start + row * stride;
            let (start, stride, modern_hash) = SETTINGS2_MODERN_ROWS;
            let modern_row = tree.s2_d.at + start + row * stride;
            // The row's provider metadata tag belongs to the row position and is constant across
            // native owners; the binding hash comes from the source, which may name an alternate.
            put(
                &mut w.out,
                native_row + hash,
                &p.bytes::<4>(modern_row + modern_hash)?,
            )?;
        }
        Ok(at)
    }

    pub(super) fn settings_pointers(&self, w: &mut Writer, at: usize) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let template = self.template;
        let template_records = self.template_records;
        let s2_i = s.record(tree.s2_d.twin)?;
        let td = template.pointer(24)?;
        // Every pointer the template's settings2 definition holds into a template object is
        // re-targeted to the object of the same role here: the roots, itself and the state.
        let template_s2 = td + ROOT_DEFINITION_SIZE + SETTINGS1_DEFINITION_SIZE;
        let template_state = template_records
            .iter()
            .find(|record| record.class == 0x808037BA)
            .context("native movement template lacks a state definition")?
            .at;
        let mut field = 0x10;
        while field + 8 <= SETTINGS2_DEFINITION_SIZE {
            let template_field = template_s2 + field;
            let value = template.u64(template_field)?;
            if value != 0 && value != u64::MAX {
                if let Ok(target) = template.pointer(template_field) {
                    if let Some(record) = template_records.iter().find(|record| record.at == target)
                    {
                        let role = match record.class {
                            0x80803B73 => tree.root_d.at,
                            0x8080388F => tree.root_i.at,
                            0x80809BD8 => tree.s2_d.at,
                            0x80809BD9 => s2_i.at,
                            0x808037BA if target == template_state => tree.states[0].at,
                            other => bail!(
                                "movement settings point at a template object of class {other:08X}"
                            ),
                        };
                        w.fixups.push(Fixup::Relative {
                            at: at + field,
                            target: role,
                        });
                    }
                }
            }
            field += 8;
        }
        Ok(())
    }

    pub(super) fn definition_blocks(&self, w: &mut Writer) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // blocks
        w.array_header(tree.blocks_headers.def, 0x8080851C, tree.blocks.len());
        for block in &tree.blocks {
            let at = w.record(
                block.holder.at,
                verbatim(p, block.holder, HOLDER_SIZE, owner_tag)?,
            );
            w.fixups.push(Fixup::Twin {
                at,
                twin: block.holder.twin,
            });
            expression_definition(w, s, &block.expression, owner_tag)?;
        }
        for block in &tree.blocks {
            expression_arrays(w, s, resources, &block.expression, owner_tag)?;
        }
        Ok(())
    }

    pub(super) fn slot_definitions(&self, w: &mut Writer) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        let reference = |tag: u32| resources.tag(tag);
        // slots
        w.array_header(tree.slots_headers.def, 0x808037CD, tree.slots.len());
        for slot in &tree.slots {
            let mut bytes = mapped(
                p,
                slot.def,
                SLOT_DEFINITION_SIZE,
                SLOT_DEFINITION,
                owner_tag,
                None,
            )?;
            // +30 names a selector resource, like the selector rows.
            put(
                &mut bytes,
                0x30,
                &reference(p.u32(slot.def.at + 0x30)?)?.to_le_bytes(),
            )?;
            // Selector and curve descriptors are rebuilt by fixups.
            put(&mut bytes, 0x18, &[0; 16])?;
            put(&mut bytes, 0x48, &[0; 16])?;
            let at = w.record(slot.def.at, bytes);
            w.fixups.push(Fixup::Twin {
                at,
                twin: slot.def.twin,
            });
            w.fixups.push(Fixup::Array {
                at: at + 0x48,
                count: slot.curves.len(),
                header: slot.curves_headers.def,
            });
        }
        Ok(())
    }

    pub(super) fn slot_arrays(&self, w: &mut Writer, slot: &Slot) -> Result<()> {
        let s = self.s;
        let resources = self.resources;
        let reference = |tag: u32| resources.tag(tag);
        let slot_at = w.placed[&slot.def.at];
        if let Some((header, rows, count)) =
            s.raw_array(slot.def.at + 0x18, 0x80809FC3, MODERN_SELECTOR_ROW)?
        {
            // A modern row is two zero words, the tag, a flag word and two more zeros; the
            // native row keeps the two zeros, the tag and one zero.
            let mut native_rows = Vec::with_capacity(count * SELECTOR_ROW);
            for row in rows.chunks_exact(MODERN_SELECTOR_ROW) {
                let tag = u32::from_le_bytes(row[8..12].try_into()?);
                ensure!(
                    row[..8] == [0; 8] && row[16..] == [0; 8],
                    "movement slot selector row carries unmapped words"
                );
                native_rows.extend_from_slice(&row[..8]);
                native_rows.extend(reference(tag)?.to_le_bytes());
                native_rows.extend([0; 4]);
            }
            w.raw_array(
                slot_at + 0x18,
                Some((header, native_rows, count)),
                0x80809FC7,
            );
        }
        w.array_header(slot.curves_headers.def, 0x808037D0, slot.curves.len());
        for curve in &slot.curves {
            self.curve_definition(w, curve)?;
        }
        for curve in &slot.curves {
            self.curve_arrays(w, curve)?;
        }
        Ok(())
    }

    pub(super) fn curve_definition(&self, w: &mut Writer, curve: &Curve) -> Result<()> {
        let s = self.s;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        let reference = |tag: u32| resources.tag(tag);
        let at = w.record(
            curve.header.at,
            verbatim(p, curve.header, CURVE_HEADER_SIZE, owner_tag)?,
        );
        w.fixups.push(Fixup::Twin {
            at,
            twin: curve.header.twin,
        });
        let mut bytes = mapped(p, curve.data, CURVE_DATA_SIZE, CURVE_DATA, owner_tag, None)?;
        // The fourth category table (+88) never occurs in the corpus and stays refused.
        for modern in (0x80..0x148).step_by(4) {
            if !matches!(modern, 0x80 | 0x100 | 0x104 | 0x118 | 0x128 | 0x134 | 0x138) {
                ensure!(
                    p.u32(curve.data.at + modern)? == 0,
                    "movement curve data uses modern-only field +{modern:X}"
                );
            }
        }
        // The modern record repeats the dictionary tag at +100; the flag word beside the
        // second copy differs and the native takes the first (+70, native +68).
        ensure!(
            p.u32(curve.data.at + 0x100)? == p.u32(curve.data.at + 0x60)?,
            "movement curve data repeats a different dictionary tag"
        );
        let tag = p.u32(curve.data.at + 0x60)?;
        put(&mut bytes, 0x60, &reference(tag)?.to_le_bytes())?;
        put(&mut bytes, 0x64, &[0; 4])?;
        put(&mut bytes, 0xA8, &[0; 0x20])?;
        let at = w.record(curve.data.at, bytes);
        w.fixups.push(Fixup::Twin {
            at,
            twin: curve.data.twin,
        });
        expression_definition(w, s, &curve.expression, owner_tag)?;
        Ok(())
    }

    pub(super) fn curve_arrays(&self, w: &mut Writer, curve: &Curve) -> Result<()> {
        let s = self.s;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        let data_at = w.placed[&curve.data.at];
        // Category tables at +18, +28 and +38, then the mask trailer the record points at
        // from +78 (native +70), which sits right behind the last table's rows.
        let rewriter = Rewriter::new(p, resources, 0);
        for descriptor in [0x18, 0x28, 0x38] {
            if let Some((header, _, count)) = s.raw_array(
                curve.data.at + descriptor,
                embedded::CATEGORY_ROW,
                embedded::MODERN_CATEGORY_ROW_SIZE,
            )? {
                let rows = rewriter.category_rows(header + 16, count)?;
                w.raw_array(
                    data_at + descriptor,
                    Some((header, rows, count)),
                    native_class(embedded::CATEGORY_ROW),
                );
            }
        }
        if p.u64(curve.data.at + 0x78)? != 0 {
            let body = p.pointer(curve.data.at + 0x78)?;
            let class = p.u32(body - 4)?;
            let trailer = rewriter.trailer(class, body)?;
            let at = w.embedded(class)?;
            w.out.extend(trailer);
            put(
                &mut w.out,
                data_at + 0x70,
                &(at as i64 - (data_at + 0x70) as i64).to_le_bytes(),
            )?;
        }
        if let Some((header, code, count)) = s.raw_array(curve.data.at + 0x148, CODE, 1)? {
            let constants = usize::try_from(p.u64(curve.data.at + 0x158)?)?;
            let inputs = usize::try_from(p.u64(curve.data.at + 0x168)?)?;
            let native = procedural::lower_program(&code, constants, inputs)
                .with_context(|| format!("movement curve program at {:X}", curve.data.at))?;
            w.raw_array(data_at + 0xA8, Some((header, native, count)), CODE);
        }
        if let Some((header, mut rows, count)) = s.raw_array(curve.data.at + 0x158, VECTORS, 16)? {
            if let Some((_, code, _)) = s.raw_array(curve.data.at + 0x148, CODE, 1)? {
                super::super::constants::broadcast(&code, &mut rows);
            }
            w.raw_array(data_at + 0xB8, Some((header, rows, count)), VECTORS);
        }
        expression_arrays(w, s, resources, &curve.expression, owner_tag)?;
        Ok(())
    }

    pub(super) fn definition_states(&self, w: &mut Writer) -> Result<()> {
        let s = self.s;
        let tree = self.tree;
        let resources = self.resources;
        let owner_tag = self.owner_tag;
        let p = s.p;
        // scalar expressions
        w.array_header(tree.scalars_headers.def, 0x808089F8, tree.scalars.len());
        for expression in &tree.scalars {
            expression_definition(w, s, expression, owner_tag)?;
        }
        for expression in &tree.scalars {
            expression_arrays(w, s, resources, expression, owner_tag)?;
        }
        // state
        w.array_header(tree.states_headers.def, 0x808037BB, tree.states.len());
        for state in &tree.states {
            let mut bytes = mapped(
                p,
                *state,
                STATE_DEFINITION_SIZE,
                STATE_DEFINITION,
                owner_tag,
                None,
            )?;
            substitute_classes(&mut bytes[0x10..]);
            let at = w.record(state.at, bytes);
            w.fixups.push(Fixup::Twin {
                at,
                twin: state.twin,
            });
        }
        // The provider array follows the state rows in both layouts without a descriptor naming
        // it: one row of a pointer to the root definition, -1, the root class and a flag word.
        let provider =
            (tree.states_headers.def + 16 + tree.states.len() * STATE_DEFINITION_SIZE + 19) & !15;
        ensure!(
            p.u64(provider)? == 1 && p.u32(provider + 8)? == MODERN_STATE_PROVIDER,
            "movement state rows are not followed by the provider row"
        );
        w.array_header(provider, native_class(MODERN_STATE_PROVIDER), 1);
        let row = w.out.len();
        w.out.extend(p.bytes::<16>(provider + 16)?);
        substitute_classes(&mut w.out[row + 8..row + 12]);
        w.fixups.push(Fixup::Relative {
            at: row,
            target: p.pointer(provider + 16)?,
        });

        Ok(())
    }
}

pub(super) fn fixups(w: &mut Writer) -> Result<()> {
    // ---- fixups ----
    for fixup in &w.fixups {
        match *fixup {
            Fixup::Twin { at, twin } => {
                let target = *w
                    .placed
                    .get(&twin)
                    .context("movement twin was not written")?;
                put(&mut w.out, at + 8, &(target as u64).to_le_bytes())?;
            }
            Fixup::Relative { at, target } => {
                let target = *w.placed.get(&target).with_context(|| {
                    format!("movement pointer at {at:X} names an unwritten object")
                })?;
                put(&mut w.out, at, &(target as i64 - at as i64).to_le_bytes())?;
            }
            Fixup::Into { at, target } => {
                let (start, new) = w
                    .placed
                    .range(..=target)
                    .next_back()
                    .map(|(start, new)| (*start, *new))
                    .context("movement pointer into an unwritten record")?;
                let new = new + (target - start);
                put(&mut w.out, at, &(new as i64 - at as i64).to_le_bytes())?;
            }
            Fixup::Array { at, count, header } => {
                put(&mut w.out, at, &(count as u64).to_le_bytes())?;
                if count == 0 {
                    put(&mut w.out, at + 8, &0u64.to_le_bytes())?;
                } else {
                    let target = *w
                        .placed
                        .get(&header)
                        .context("movement array header was not written")?;
                    put(
                        &mut w.out,
                        at + 8,
                        &(target as i64 - (at + 8) as i64).to_le_bytes(),
                    )?;
                }
            }
        }
    }
    Ok(())
}
