//! Source layouts from HKLib ac40bc9915d8caff0df154c2b9a79656dfca1d77.
//! Its MIT notice is retained beside the reduced schema. Native layouts and
//! class signatures come from Shadowkeep 86657.20.08.23.1800.d2_rc___release.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Schema {
    pub concrete: BTreeMap<u32, u32>,
    pub types: BTreeMap<u32, SourceType>,
    #[serde(skip)]
    pub native: BTreeMap<String, NativeType>,
}

#[derive(Deserialize)]
pub(super) struct SourceType {
    pub name: String,
    pub parent: u32,
    pub format: u32,
    pub subtype: u32,
    pub size: usize,
    pub members: Vec<SourceMember>,
}

#[derive(Clone, Deserialize)]
pub(super) struct SourceMember {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u32,
    pub offset: usize,
    pub flags: u32,
}

#[derive(Deserialize)]
pub(super) struct NativeType {
    pub size: usize,
    pub parent: Option<String>,
    pub signature: Option<u32>,
    pub members: Vec<Member>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Member {
    pub name: String,
    pub kind: u8,
    pub subtype: u8,
    pub length: usize,
    pub flags: u32,
    pub offset: usize,
    pub class: Option<String>,
}

impl Schema {
    pub fn read() -> Result<Self> {
        let mut result: Self = serde_json::from_str(include_str!("schema/source.json"))?;
        result.native = serde_json::from_str(include_str!("schema/native.json"))?;
        Ok(result)
    }

    pub fn source(&self, id: u32) -> Result<&SourceType> {
        self.types
            .get(&id)
            .with_context(|| format!("Unknown cloth type {id}"))
    }

    pub fn subtype(&self, id: u32) -> Result<u32> {
        let row = self.source(id)?;
        if row.subtype != 0 || row.parent == 0 {
            Ok(row.subtype)
        } else {
            self.subtype(row.parent)
        }
    }

    pub fn type_name(&self, id: u32) -> Result<String> {
        let row = self.source(id)?;
        let sub = self.subtype(id)?;
        if sub != 0
            && matches!(
                row.name.as_str(),
                "hkArray" | "T*" | "hkRefPtr" | "hkViewPtr" | "T[N]"
            )
        {
            Ok(format!("{}<{}>", row.name, self.type_name(sub)?))
        } else {
            Ok(row.name.clone())
        }
    }

    pub fn source_members(&self, id: u32) -> Result<Vec<SourceMember>> {
        let row = self.source(id)?;
        let mut fields = if row.parent != 0 && row.format & 15 == 7 {
            self.source_members(row.parent)?
        } else {
            Vec::new()
        };
        fields.extend(row.members.iter().filter(|m| m.flags & 1 == 0).cloned());
        Ok(fields)
    }

    pub fn native(&self, name: &str) -> Result<&NativeType> {
        self.native
            .get(name)
            .with_context(|| format!("Unsupported native cloth type {name}"))
    }

    pub fn members(&self, name: &str) -> Result<Vec<Member>> {
        let row = self.native(name)?;
        let mut fields = match &row.parent {
            Some(parent) => self.members(parent)?,
            None => Vec::new(),
        };
        fields.extend(row.members.iter().filter(|m| m.flags & 1024 == 0).cloned());
        Ok(fields)
    }

    pub fn size(&self, kind: u8, class: Option<&str>) -> Result<usize> {
        Ok(match kind {
            1..=4 => 1,
            5 | 6 | 30 => 2,
            7 | 8 | 11 | 32 => 4,
            9 | 10 | 20 | 21 | 26 | 29 | 31 | 33 => 8,
            12 | 13 | 22 => 16,
            14..=16 => 48,
            17 | 18 => 64,
            25 => self.native(class.context("Native struct class")?)?.size,
            _ => anyhow::bail!("Unsupported native cloth field kind {kind}"),
        })
    }
}
