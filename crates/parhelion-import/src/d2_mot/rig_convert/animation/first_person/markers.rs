//! Runtime weapon markers drive arm constraints independently of art-part markers.
use super::*;
use crate::d2_mot::markers as format;

fn owner(rig: &Value, class: &str) -> Result<Option<u32>> {
    let owners = rig["components"]
        .as_array()
        .context("runtime marker components")?
        .iter()
        .filter(|c| c["entity"] == rig["runtime_entity"] && c["class"] == class)
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    ensure!(owners.len() <= 1, "runtime marker owner is ambiguous");
    Ok(owners.first().copied())
}

/// Check the native component and every marker before package allocation.
pub fn validate(bytes: &[u8]) -> Result<()> {
    let p = Payload(bytes.to_vec());
    let instance = p.pointer(16)?;
    let resource = p.pointer(24)?;
    ensure!(
        p.u64(0)? == bytes.len() as u64
            && instance >= 4
            && resource >= 4
            && p.u32(instance - 4)? == format::NATIVE_COMPONENT
            && p.u32(resource - 4)? == 0x80808507,
        "runtime marker component layout differs"
    );
    for marker in format::read_native(&p)? {
        ensure!(
            marker.binding[0] == 0
                && marker.binding[1] <= 1
                && marker.position.iter().all(|x| x.is_finite())
                && marker.orientation.iter().all(|x| x.is_finite()),
            "runtime marker transform or root binding differs"
        );
    }
    Ok(())
}

pub(super) fn prepare(
    sr: &mut Reader,
    nr: &mut Reader,
    source: &Value,
    native: &Value,
    graph: &Path,
) -> Result<Option<Value>> {
    let Some(from) = owner(source, "8080819D")? else {
        return Ok(None);
    };
    let to = owner(native, "80808507")?
        .context("source runtime markers need a native component carrier")?;
    let modern = sr.tag(from, Some(0x80809B06))?;
    let original = nr.tag(to, Some(0x80809C36))?;
    validate(&original.0)?;
    let resource = modern.pointer(24)?;
    ensure!(
        resource >= 4 && modern.u32(resource - 4)? == 0x8080819D,
        "source runtime marker resource layout differs"
    );
    let rows = format::read(&modern, resource + 0xB8, format::SOURCE)?;
    let converted = format::replace(&original, &rows)?;
    validate(&converted.0)?;
    let actual = format::read_native(&converted)?;
    ensure!(
        actual.len() == rows.len()
            && actual.iter().zip(&rows).all(|(a, b)| a.name == b.name
                && a.binding == b.binding
                && a.position == b.position
                && a.orientation == b.orientation),
        "source runtime markers changed during serialization"
    );
    let file = format!("animation/markers-{from:08X}.bin");
    let template_file = format!("animation/markers-{from:08X}-template.bin");
    fs::create_dir_all(graph.join("animation"))?;
    fs::write(graph.join(&file), &converted.0)?;
    fs::write(graph.join(&template_file), &original.0)?;
    Ok(Some(json!({"kind":"markers","source":from,"native":to,
        "first_person":false,"file":file,"template_file":template_file,
        "count":rows.len(),"gameplay_verified":false})))
}

/// Refresh already prepared source-owned rigs without changing their working clips.
pub fn refresh(
    sr: &mut Reader,
    nr: &mut Reader,
    source: &Value,
    native: &Value,
    directory: &Path,
    graph: &mut Value,
) -> Result<()> {
    let rigs = graph["animation"]["first_person"]["rigs"]
        .as_array_mut()
        .context("prepared source rig owners")?;
    ensure!(
        rigs.iter()
            .any(|r| r["first_person"] == false
                && (r.get("kind").is_none() || r["kind"] == "skeleton")),
        "runtime marker refresh requires a source-owned weapon skeleton"
    );
    let converted = prepare(sr, nr, source, native, directory)?;
    rigs.retain(|rig| rig["kind"] != "markers");
    rigs.extend(converted);
    Ok(())
}
