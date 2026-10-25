//! Match stored Xbox stream declarations to actual native declarations.
use super::*;
use crate::tiger::reader::Reader;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    semantic: u8,
    index: u8,
    format: u8,
}

type Streams = [Vec<Element>; 4];

pub(super) struct Layouts {
    source: BTreeMap<u16, Streams>,
    native: BTreeMap<u16, Streams>,
}

fn declarations(
    elements: &Payload,
    mapping: &Payload,
    source: bool,
) -> Result<BTreeMap<u16, Streams>> {
    let elements = elements
        .array(8, 16, None)?
        .into_iter()
        .map(|row| {
            elements
                .array(row, 3, None)?
                .into_iter()
                .map(|at| {
                    let format = elements.u8(at + 2)?;
                    Ok(Element {
                        semantic: elements.u8(at)?,
                        index: elements.u8(at + 1)?,
                        format: if source {
                            match format {
                                9 => 10,
                                10 => 11,
                                11 => 12,
                                other => other,
                            }
                        } else {
                            format
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut result = BTreeMap::new();
    for row in mapping.array(8, 28, None)? {
        // The armor path does not consume hardware-instanced declarations.
        if mapping.bytes::<4>(row + 24)? != [0; 4] {
            continue;
        }
        let mut streams: Streams = std::array::from_fn(|_| Vec::new());
        for (index, stream) in streams.iter_mut().enumerate() {
            let selected = mapping.u32(row + 8 + index * 4)?;
            if selected != u32::MAX {
                *stream = elements
                    .get(selected as usize)
                    .context("Vertex element set is absent")?
                    .clone();
            }
        }
        ensure!(
            result.insert(mapping.u16(row)?, streams).is_none(),
            "Repeated stream layout identity"
        );
    }
    Ok(result)
}

impl Source {
    pub(super) fn named(&self, name: &str) -> Result<u32> {
        let tags = self
            .packages
            .values()
            .flat_map(|package| package.reader.named_tags())
            .filter(|tag| tag.name == name)
            .map(|tag| tag.hash.0)
            .collect::<std::collections::BTreeSet<_>>();
        ensure!(tags.len() == 1, "Missing or ambiguous source name {name}");
        Ok(*tags.first().unwrap())
    }
}

impl Layouts {
    pub fn read(source: &Source, native: &mut Reader) -> Result<Self> {
        let bootstrap = source.read(source.named("client_bootstrap_patchable")?)?;
        let globals = source.read(bootstrap.u32(0x3C)?)?;
        let root = source.read(globals.u32(8)?)?;
        let elements = source.read(root.u32(0x10)?)?;
        let mappings = source.read(root.u32(0x28)?)?;
        let source = declarations(&elements, &mappings, true)?;
        let named = native
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|tag| tag.name == "client_bootstrap_patchable")
            .context("Native bootstrap name is absent")?
            .hash
            .0;
        let bootstrap = native.tag(named, None)?;
        let globals = native.tag(bootstrap.u32(0x4C)?, None)?;
        let root = native.tag(globals.u32(8)?, Some(0x808072A6))?;
        let elements = native.tag(root.u32(12)?, None)?;
        let mappings = native.tag(root.u32(40)?, None)?;
        Ok(Self {
            source,
            native: declarations(&elements, &mappings, false)?,
        })
    }

    pub fn matching(&self, source: u8, strides: [usize; 4]) -> Result<u16> {
        let layout = self
            .source
            .get(&u16::from(source))
            .context("Missing source stream declaration")?;
        for (stream, stride) in layout.iter().zip(strides) {
            let mut size = 0;
            for element in stream {
                size += match element.format {
                    4 => 16,
                    5 | 6 | 10 | 12 => 4,
                    11 => 8,
                    other => anyhow::bail!("Unsupported source vertex element format {other}"),
                };
            }
            ensure!(
                size == stride,
                "Source declaration and stored vertex stride differ"
            );
        }
        self.native
            .iter()
            .find(|(_, candidate)| *candidate == layout)
            .map(|(index, _)| *index)
            .context("Source stream declaration has no byte-compatible native layout")
    }
}
