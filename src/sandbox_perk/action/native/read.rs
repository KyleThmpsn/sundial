use super::*;
use crate::package_payload::{i64_at, relative_offset, u32_at, u64_at};

struct Reader<'a> {
    data: &'a [u8],
    graph: Graph,
    seen: BTreeMap<(usize, u32, Option<usize>), usize>,
    total: usize,
}

pub(super) fn capture(data: &[u8], offset: usize, class: u32) -> Result<Graph, String> {
    if data.len() > MAX_BYTES {
        return Err("The native resource exceeds the editor size limit.".into());
    }
    let mut reader = Reader {
        data,
        graph: Graph { blocks: Vec::new() },
        seen: BTreeMap::new(),
        total: 0,
    };
    reader.visit(offset, class, None, 0)?;
    reader.graph.validate()?;
    Ok(reader.graph)
}

impl Reader<'_> {
    fn visit(
        &mut self,
        offset: usize,
        class: u32,
        count: Option<usize>,
        depth: usize,
    ) -> Result<usize, String> {
        if let Some(index) = self.seen.get(&(offset, class, count)) {
            return Ok(*index);
        }
        if self.graph.blocks.len() >= MAX_BLOCKS || depth > 64 {
            return Err("The native graph exceeds its nesting or allocation limit.".into());
        }
        let size = if class == 0 {
            self.string_size(offset)?
        } else {
            schema::record(class)?
                .size
                .checked_mul(count.unwrap_or(1))
                .ok_or("Native allocation size overflow.")?
        };
        if count.is_some_and(|n| n > MAX_ROWS) || size > MAX_BYTES {
            return Err("The native allocation is too large.".into());
        }
        let end = offset
            .checked_add(size)
            .ok_or("Native allocation offset overflow.")?;
        let bytes = self
            .data
            .get(offset..end)
            .ok_or("A native allocation exceeds the resource.")?
            .to_vec();
        self.total = self
            .total
            .checked_add(size)
            .ok_or("Native graph size overflow.")?;
        if self.total > MAX_BYTES {
            return Err("The native graph is too large.".into());
        }
        let index = self.graph.blocks.len();
        self.seen.insert((offset, class, count), index);
        self.graph.blocks.push(Block {
            class,
            count,
            bytes,
            links: BTreeMap::new(),
        });
        if class != 0 {
            let record = schema::record(class)?;
            for row in 0..count.unwrap_or(1) {
                for &(field, code) in &record.fields {
                    if matches!(code, 1..=3) {
                        self.pointer(index, offset, row * record.size + field, code, depth)?;
                    }
                }
            }
        }
        Ok(index)
    }

    fn string_size(&self, offset: usize) -> Result<usize, String> {
        let tail = self
            .data
            .get(offset..)
            .ok_or("A native string exceeds its resource.")?;
        let length = tail
            .iter()
            .take(65536)
            .position(|b| *b == 0)
            .ok_or("A native string has no terminator.")?;
        std::str::from_utf8(&tail[..length]).map_err(|_| "A native string is not UTF-8.")?;
        Ok(length + 1)
    }

    fn pointer(
        &mut self,
        index: usize,
        base: usize,
        field: usize,
        code: u32,
        depth: usize,
    ) -> Result<(), String> {
        let absolute = base
            .checked_add(field)
            .ok_or("Native pointer offset overflow.")?;
        let relative = i64_at(self.data, absolute)?;
        if relative == 0 {
            return Ok(());
        }
        let target = relative_offset(absolute, 0, relative)?;
        let child = if code == 3 {
            self.typed(absolute, target, depth + 1)?
        } else {
            self.visit(target, 0, None, depth + 1)?
        };
        if child == 0 {
            return Err("A native node points back to its allocation root.".into());
        }
        self.graph.blocks[index].links.insert(field, child);
        self.graph.blocks[index]
            .bytes
            .get_mut(field..field + 8)
            .ok_or("A native pointer exceeds its object.")?
            .fill(0);
        Ok(())
    }

    fn typed(&mut self, field: usize, target: usize, depth: usize) -> Result<usize, String> {
        let marker = target
            .checked_sub(4)
            .ok_or("A native pointer has no class header.")?;
        let class = u32_at(self.data, marker)?;
        if class != 0x80809FBD {
            return self.visit(target, class, None, depth);
        }
        let count = usize::try_from(u64_at(self.data, target)?)
            .map_err(|_| "Native array length overflow.")?;
        if u32_at(self.data, target + 12)? != 0
            || field < 8
            || u64_at(self.data, field - 8)? != count as u64
        {
            return Err("Native array header and descriptor disagree.".into());
        }
        let class = u32_at(self.data, target + 8)?;
        if schema::record(class)?.size == 0 {
            return Err("A native array has zero stride.".into());
        }
        self.visit(target + 16, class, Some(count), depth)
    }
}
