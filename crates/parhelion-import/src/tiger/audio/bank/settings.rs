//! Init definitions needed by imported banks, without replacing native globals.
use super::*;

#[derive(Default)]
pub(super) struct Dependencies {
    pub groups: BTreeSet<u32>,
    pub switches: BTreeSet<u32>,
    pub params: BTreeSet<u32>,
}

#[derive(PartialEq, Eq)]
pub struct Settings {
    prefix: [u8; 6],
    groups: BTreeMap<u32, Vec<u8>>,
    switches: BTreeMap<u32, Vec<u8>>,
    params: BTreeMap<u32, Vec<u8>>,
}

impl Settings {
    pub fn read(bytes: &[u8], version: u32) -> Result<Self> {
        ensure!(matches!(version, 113 | 150), "unsupported Init version");
        ensure!(bytes.len() <= 64 * 1024 * 1024, "Init bank exceeds limit");
        let mut r = Read { bytes, at: 0 };
        let mut header = None;
        let mut body = None;
        while r.at < bytes.len() {
            let kind = r.take(4)?;
            let size = r.u32()? as usize;
            let value = r.take(size)?;
            match kind {
                b"BKHD" => ensure!(header.replace(value).is_none(), "duplicate Init header"),
                b"STMG" => ensure!(body.replace(value).is_none(), "duplicate Init settings"),
                _ => {}
            }
        }
        let mut h = Read {
            bytes: header.context("Init header missing")?,
            at: 0,
        };
        ensure!(h.u32()? == version, "Init version mismatch");
        ensure!(h.u32()? == hash("init"), "settings bank is not Init");
        let mut r = Read {
            bytes: body.context("Init settings missing")?,
            at: 0,
        };
        if version == 150 {
            r.u16()?; // Source-only filter behavior.
        }
        let prefix = r.take(6)?.try_into()?;
        if version == 150 {
            r.u16()?; // Source-only dangerous voice limit.
        }
        let mut groups = BTreeMap::new();
        let count = r.count()?;
        for _ in 0..count {
            let start = r.at;
            let id = r.u32()?;
            r.u32()?;
            let transitions = r.count()?;
            r.take(transitions * 12)?;
            ensure!(
                groups.insert(id, r.bytes[start..r.at].to_vec()).is_none(),
                "duplicate Init state group"
            );
        }
        let mut switches = BTreeMap::new();
        let count = r.count()?;
        for _ in 0..count {
            let start = r.at;
            let id = r.u32()?;
            r.take(5)?;
            let points = r.count()?;
            r.take(points * 12)?;
            ensure!(
                switches.insert(id, r.bytes[start..r.at].to_vec()).is_none(),
                "duplicate Init switch group"
            );
        }
        let mut params = BTreeMap::new();
        let count = r.count()?;
        for _ in 0..count {
            let row = r.take(21)?;
            let id = u32::from_le_bytes(row[..4].try_into()?);
            ensure!(
                params.insert(id, row.to_vec()).is_none(),
                "duplicate Init parameter"
            );
        }
        if version == 150 {
            let textures = r.count()?;
            r.take(textures * 28)?;
        }
        r.end()?;
        Ok(Self {
            prefix,
            groups,
            switches,
            params,
        })
    }

    pub(super) fn supplement(&self, native: &Self, deps: &Dependencies) -> Result<Option<Vec<u8>>> {
        let groups = deps
            .groups
            .iter()
            .filter(|id| !native.groups.contains_key(id))
            .copied()
            .collect::<BTreeSet<_>>();
        let switches = deps
            .switches
            .iter()
            .filter(|id| !native.switches.contains_key(id) && self.switches.contains_key(id))
            .copied()
            .collect::<BTreeSet<_>>();
        let mut params = deps.params.clone();
        for id in &switches {
            let row = &self.switches[id];
            ensure!(
                row[8] == 0,
                "Init switch {id:08X} needs unsupported controller"
            );
            params.insert(u32::from_le_bytes(row[4..8].try_into()?));
        }
        params.retain(|id| !native.params.contains_key(id) && self.params.contains_key(id));
        if groups.is_empty() && switches.is_empty() && params.is_empty() {
            return Ok(None);
        }
        ensure!(
            self.prefix == native.prefix,
            "source and native mixer limits differ"
        );
        for id in &params {
            ensure!(
                self.params[id][20] == 0,
                "Init parameter {id:08X} needs unsupported builtin binding"
            );
        }
        let mut out = Write::default();
        out.bytes.extend(native.prefix);
        for (ids, rows) in [
            (&groups, &self.groups),
            (&switches, &self.switches),
            (&params, &self.params),
        ] {
            out.u32(u32::try_from(ids.len())?);
            for id in ids {
                out.bytes.extend(
                    rows.get(id)
                        .with_context(|| format!("source Init definition {id:08X} missing"))?,
                );
            }
        }
        Ok(Some(out.bytes))
    }
}
