//! Recreate native rendering contracts from the user's packages, never old exports.
use super::*;
use crate::d2_mot::tfx;

#[cfg(test)]
mod tests;

pub(super) fn link_inputs(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let to = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if entry.file_name() == "raw" {
                link_inputs(&entry.path(), &to)?;
            }
        } else if !to.exists() {
            fs::hard_link(entry.path(), &to)
                .or_else(|_| fs::copy(entry.path(), &to).map(|_| ()))?;
        }
    }
    Ok(())
}

pub(super) fn export(prepared: &Path, root: &Path, progress: &mut dyn FnMut(String)) -> Result<()> {
    progress("Reading native shader and effect contracts…".into());
    let native = load(&prepared.join("native/source-manifest.json"))?;
    let modern = load(&prepared.join("source/source-manifest.json"))?;
    let native = Path::new(native["packages"].as_str().context("Native packages")?);
    let modern = Path::new(modern["packages"].as_str().context("Modern packages")?);
    let mut native_reader = Reader::new(native, &root.join("native"), false)?;
    let mut modern_reader = Reader::new(modern, &root.join("modern"), true)?;
    super::super::contracts::export(&mut native_reader, native, root, progress)?;
    for (folder, is_modern) in [("tfx-native", false), ("tfx-modern", true)] {
        let reader = if is_modern {
            &mut modern_reader
        } else {
            &mut native_reader
        };
        reader.begin_export(&root.join(folder))?;
        let context = tfx::context(reader, is_modern)?;
        write_json(&reader.output.join("context.json"), &context)?;
        reader.finish()?;
    }
    let reader = &mut native_reader;
    for (folder, tag) in [
        ("native-procedure-bank-02", 0x81532E86),
        ("native-procedure-provider-02", 0x80FC5E94),
        ("native-procedure-entity-01", 0x80C4B3B7),
    ] {
        reader.begin_export(&root.join(folder))?;
        let p = reader.tag(tag, None)?;
        if folder == "native-procedure-bank-02" {
            reader.tag(p.u32(0x44)?, None)?;
        }
        reader.finish()?;
    }
    Ok(())
}
