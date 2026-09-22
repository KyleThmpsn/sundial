use crate::d2_mot::reader::Reader;
use anyhow::Result;
use serde_json::{Value, json};
pub fn inspect(r: &mut Reader, tag: u32) -> Result<Value> {
    let p = r.tag(tag, None)?;
    let mut references = vec![];
    for o in (0..p.0.len().saturating_sub(3)).step_by(4) {
        let t = p.u32(o)?;
        let h = tiger_pkg::TagHash(t);
        if tiger_pkg::TagHash::new(h.pkg_id(), h.entry_index()).0 != t {
            continue;
        }
        if let Some(e) = r.manager.get_entry(tiger_pkg::TagHash(t)) {
            references.push(json!({"offset":format!("{o:X}"),"tag":format!("{t:08X}"),"class":format!("{:08X}",e.reference),"type":e.file_type,"subtype":e.file_subtype}));
            r.tag(t, None)?;
        }
    }
    Ok(
        json!({"tag":format!("{tag:08X}"),"bytes":hex::encode(&p.0),"candidate_references":references}),
    )
}
