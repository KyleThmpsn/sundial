//! Bind the bounded source query shape to its matching native component and dispatch records.
use super::*;

const TEMPLATES: [(u32, u32); 2] = [(0x81522CBA, 0x80BB890A), (0x8152C721, 0x80BB893B)];

fn predicates(p: &Payload, source: bool) -> Result<Value> {
    let r = p.pointer(24)?;
    let mut groups = Vec::new();
    for row in p.array(
        r + if source { 0x98 } else { 0x80 },
        24,
        Some(if source { 0x808030AA } else { 0x80803832 }),
    )? {
        let kind = p.pointer(row)?;
        ensure!(
            p.u32(kind - 4)? == if source { 0x808030F3 } else { 0x80803838 },
            "Query group implementation differs"
        );
        let mut filters = Vec::new();
        for filter in p.array(
            row + 8,
            32,
            Some(if source { 0x80803182 } else { 0x80803835 }),
        )? {
            let mut terms = Vec::new();
            for term in p.array(
                filter + 16,
                16,
                Some(if source { 0x808003AE } else { 0x80809316 }),
            )? {
                let value = p.pointer(term + 8)?;
                let kind = match (source, p.u32(value - 4)?) {
                    (true, 0x8080295D) | (false, 0x80804D76) => 0,
                    (true, 0x808026B3) | (false, 0x80804D7A) => 1,
                    _ => anyhow::bail!("Unsupported source query predicate"),
                };
                let words = if kind == 0 {
                    vec![p.u32(value)?]
                } else {
                    vec![p.u32(value)?, p.u32(value + 4)?]
                };
                terms.push(json!([p.u64(term)?, kind, words]));
            }
            filters.push(json!([p.bytes::<16>(filter)?, terms]));
        }
        groups.push(json!([p.u32(kind)?, filters]));
    }
    Ok(json!(groups))
}

impl Assembly<'_> {
    pub(super) fn query(
        &mut self,
        source: &Payload,
        name: &str,
        index: usize,
    ) -> Result<Component> {
        let queries = predicates(source, true)?;
        let mut selected = None;
        for (owner, entity) in TEMPLATES {
            let native = self.native.tag(owner, Some(0x80809C36))?;
            if queries == predicates(&native, false)? {
                selected = Some((owner, entity));
                break;
            }
        }
        let (owner, entity) =
            selected.context("Source query predicates need another native adapter")?;
        let mut component = self.component(owner, entity, name, index)?;
        let native = &component.template;
        let (s, n) = (source.pointer(24)?, native.pointer(24)?);
        ensure!(
            source.u32(s - 4)? == 0x80803136 && native.u32(n - 4)? == 0x80803820,
            "Query resource class differs"
        );
        ensure!(
            source.bytes::<8>(s + 0x90)? == native.bytes::<8>(n + 0x78)?
                && source.bytes::<24>(s + 0xA8)? == [0; 24]
                && native.bytes::<24>(n + 0x90)? == [0; 24]
                && source.bytes::<24>(s + 0xD8)? == native.bytes::<24>(n + 0xB8)?
                && source.bytes::<24>(s + 0x168)? == [0; 24],
            "Source query parameters need another native adapter"
        );
        ensure!(
            queries == predicates(native, false)?,
            "Source query predicates need translation"
        );
        let outputs = self.query_outputs(source, native)?;
        activation(source, &mut component.payload)?;
        component.seal()?;
        self.evidence.push(json!({"source_component":source.u32(source.pointer(16)?)?,
            "resource_class":0x80803136u32,"native_resource_class":0x80803820u32,
            "query_adapter":{"predicates":queries,"output_names":outputs,"parameters":"equal to checked native adapter"},
            "gameplay_verified":false}));
        Ok(component)
    }

    fn query_outputs(&mut self, source: &Payload, native: &Payload) -> Result<Vec<u32>> {
        let (s, n) = (source.pointer(24)?, native.pointer(24)?);
        let mut outputs = Vec::new();
        for (sf, nf, si, ni, sc, nc, sm, nm, embedded) in [
            (
                0x60, 0x48, 0x80800581, 0x80809ADE, 0x808008FE, 0x80809ADD, 1, 0, false,
            ),
            (
                0x78, 0x60, 0x8080059E, 0x808097B9, 0x80800835, 0x808097B8, 3, 2, false,
            ),
            (
                0xF0, 0xD0, 0x808007E9, 0x808097BB, 0x808004CD, 0x808097BA, 1, 0, true,
            ),
            (
                0x108, 0x118, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 4, 3, false,
            ),
            (
                0x128, 0x138, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 5, 4, false,
            ),
            (
                0x148, 0x158, 0x8080053B, 0x80809AE1, 0x80800539, 0x80809AE2, 6, 5, false,
            ),
        ] {
            ensure!(
                source.pointer(s + sf)? == s + if embedded { 0xC0 } else { 0 }
                    && native.pointer(n + nf)? == n + if embedded { 0xA8 } else { 0 },
                "Query provider points outside its definition"
            );
            let a = self.source.read(source.u32(s + sf + 8)?)?;
            let b = self.native.tag(native.u32(n + nf + 8)?, None)?;
            for (p, interface, instance, implementation, method, row_class) in [
                (
                    &a,
                    si,
                    sc,
                    if embedded { 0x80802BF3 } else { 0x80803143 },
                    sm,
                    0x808005C0,
                ),
                (
                    b.as_ref(),
                    ni,
                    nc,
                    if embedded { 0x80804345 } else { 0x8080381F },
                    nm,
                    0x80809C56,
                ),
            ] {
                let rows = p.array(16, 24, Some(row_class))?;
                ensure!(
                    p.u32(8)? == interface
                        && p.u32(12)? == instance
                        && rows.len() == 1
                        && p.u32(rows[0])? == implementation
                        && p.u32(rows[0] + 4)? == method
                        && p.bytes::<16>(rows[0] + 8)? == [0; 16],
                    "Query provider dispatch differs"
                );
            }
            if sf >= 0x108 {
                let output = source.u32(s + sf + 24)?;
                ensure!(
                    output == native.u32(n + nf + 24)?,
                    "Query output name differs"
                );
                outputs.push(output);
            }
        }
        Ok(outputs)
    }
}
