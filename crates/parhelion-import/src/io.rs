//! Checked output paths and import receipts.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

// Resolve junctions in the existing ancestor before creating any directories.
pub fn resolved(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut tail = vec![];
    while !ancestor.exists() {
        tail.push(
            ancestor
                .file_name()
                .context("invalid output path")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("no output ancestor")?;
    }
    let mut result = ancestor.canonicalize()?;
    for part in tail.into_iter().rev() {
        result.push(part)
    }
    Ok(result)
}
pub fn outside(output: &Path, source: &Path) -> Result<PathBuf> {
    let output = resolved(output)?;
    let source = resolved(source)?;
    ensure!(
        !output.starts_with(&source),
        "output must be outside source tree: {}",
        source.display()
    );
    Ok(output)
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    crate::cancellation::check()?;
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_source_before_creation() {
        let t = tempfile::tempdir().unwrap();
        let child = t.path().join("not-created/raw");
        assert!(outside(&child, t.path()).is_err());
        assert!(!child.exists());
    }
}
