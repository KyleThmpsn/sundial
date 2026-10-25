//! Wire descriptors also expose native offsets and scalar storage, but are not
//! package serializers. Preserve their array and presence metadata separately.
use super::*;

pub(super) type Codecs = BTreeMap<u32, Declaration>;
static CODECS: OnceLock<Result<Codecs, String>> = OnceLock::new();

#[derive(Clone, Debug)]
pub(super) struct Declaration {
    pub size: usize,
    pub array_len: usize,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    pub advance: usize,
    pub code: u8,
    pub presence: bool,
    pub child: u32,
    pub params: [u32; 4],
}

pub(super) fn registry() -> Result<&'static Codecs, String> {
    CODECS.get_or_init(load).as_ref().map_err(Clone::clone)
}

pub(super) fn locations(
    declaration: Option<&Declaration>,
    size: usize,
    active: Option<usize>,
) -> Result<Vec<(usize, &Field)>, String> {
    let Some(declaration) = declaration else {
        return if active.is_some() {
            Err("Dynamic native count has no array declaration".into())
        } else {
            Ok(Vec::new())
        };
    };
    if declaration.size != size {
        return Err("Native codec and structure sizes disagree".into());
    }
    declaration.locations(active)
}

impl Field {
    pub fn description(&self) -> String {
        format!(
            "Wire operation {}, {}. Declared type 0x{:08X}",
            self.code,
            if self.presence {
                "network presence flag"
            } else {
                "no network presence flag"
            },
            self.child,
        )
    }

    pub fn active_count(
        &self,
        data: &[u8],
        start: usize,
        size: usize,
    ) -> Result<Option<usize>, String> {
        match self.params[0] {
            0 => Ok(None),
            1 => {
                let offset = self.params[1] as usize;
                if offset.checked_add(4).is_none_or(|last| last > size) {
                    return Err("Dynamic native count exceeds its declaring structure".into());
                }
                Ok(Some(read_u32(data, start + offset)? as usize))
            }
            _ => Err("Unknown native nested-count operation".into()),
        }
    }
}

fn load() -> Result<Codecs, String> {
    type Encoded = (u32, usize, usize, Vec<[u32; 10]>);
    let encoded: Vec<Encoded> = serde_json::from_str(include_str!("codecs.json"))
        .map_err(|error| format!("Native field declarations are invalid: {error}"))?;
    let mut result = BTreeMap::new();
    for (handle, size, array_len, encoded) in encoded {
        let mut fields = Vec::new();
        for words in encoded {
            if words[0] as usize > size || words[4] & !0x1FF != 0 {
                return Err("Invalid native field descriptor".into());
            }
            fields.push(Field {
                advance: words[0] as usize,
                code: words[4] as u8,
                presence: words[4] & 0x100 != 0,
                child: words[5],
                params: words[6..10].try_into().unwrap(),
            });
        }
        if array_len != 0
            && (fields.len() != 1
                || fields[0].advance == 0
                || fields[0].advance.checked_mul(array_len) != Some(size))
        {
            return Err("Invalid native fixed array declaration".into());
        }
        if result
            .insert(
                handle,
                Declaration {
                    size,
                    array_len,
                    fields,
                },
            )
            .is_some()
        {
            return Err("Duplicate native field declaration".into());
        }
    }
    Ok(result)
}

impl Declaration {
    pub fn value_end(&self, at: usize, end: usize, field: &Field) -> Result<usize, String> {
        if self.array_len == 0 {
            return Ok(end);
        }
        at.checked_add(field.advance)
            .filter(|last| *last <= end)
            .ok_or_else(|| "Native array element exceeds its declaring structure".into())
    }

    /// On an array declaration `advance` is a stride, including at index zero.
    pub fn locations(&self, active: Option<usize>) -> Result<Vec<(usize, &Field)>, String> {
        if self.array_len == 0 {
            if let Some(count) = active {
                // Native capacity-one regions use a single field at offset zero
                // and omit the fixed-array loop. This includes both scalar and
                // nested-record optionals. A larger count cannot fit that region.
                if self.fields.len() != 1 || self.fields[0].advance != 0 {
                    return Err("Dynamic native count targets a non-array declaration".into());
                }
                if count > 1 {
                    return Err("Dynamic native count exceeds its single-value capacity".into());
                }
                if count == 0 {
                    return Ok(Vec::new());
                }
            }
            return Ok(self
                .fields
                .iter()
                .map(|field| (field.advance, field))
                .collect());
        }
        let count = active.unwrap_or(self.array_len);
        if count > self.array_len {
            return Err("Dynamic native count exceeds its array capacity".into());
        }
        if count > MAX_FIELDS {
            return Err("Native fixed array exceeds the inspection limit".into());
        }
        Ok((0..count)
            .map(|index| (index * self.fields[0].advance, &self.fields[0]))
            .collect())
    }
}

/// A known wire operation need not describe the native storage of its value.
pub(super) fn unresolved(code: u8) -> &'static str {
    match code {
        0 | 17 | 32 | 33 | 36 => "Not Serialized",
        18 | 20 | 30 | 31 => "Runtime Reference",
        19 => "Entity Reference",
        21..=23 => "Nullable Identifier",
        24 | 25 => "Runtime Union",
        26 | 27 => "Nullable Resource",
        28 | 29 | 43 => "Compound Runtime Reference",
        34 | 35 | 37 | 38 | 41 => "Runtime-Selected Structure",
        44..=46 => "Encoded Numeric Value",
        _ => "Unknown Operation",
    }
}
