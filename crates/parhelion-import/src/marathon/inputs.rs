//! Authored renderer inputs retained as editable native material constants.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Target authoring values, not a capture of Marathon's environment state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialInputs {
    pub wetness: f32,
    pub pattern: [[f32; 4]; 8],
    #[serde(default = "default_specular_aa")]
    pub specular_aa: f32,
}

fn default_specular_aa() -> f32 {
    1.0
}

impl Default for MaterialInputs {
    fn default() -> Self {
        Self {
            wetness: 0.0,
            specular_aa: default_specular_aa(),
            pattern: [
                [1.0, 0.0, 0.0, 0.125],
                [0.0, 1.0, 0.0, -0.125],
                [0.0, 0.0, 1.0, 0.25],
                [0.6, 0.8, 0.0, -0.25],
                [0.0, 0.6, 0.8, 0.375],
                [0.8, 0.0, 0.6, -0.375],
                [-0.6, 0.8, 0.0, 0.5],
                [0.0, -0.6, 0.8, -0.5],
            ],
        }
    }
}

// Recipe identity uses exact float bits, including signed zero. Validation rejects
// nonfinite values before they can reach the material evaluator.
impl PartialEq for MaterialInputs {
    fn eq(&self, other: &Self) -> bool {
        self.wetness.to_bits() == other.wetness.to_bits()
            && self.specular_aa.to_bits() == other.specular_aa.to_bits()
            && self
                .pattern
                .iter()
                .flatten()
                .zip(other.pattern.iter().flatten())
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}
impl Eq for MaterialInputs {}

impl MaterialInputs {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.wetness.is_finite() && (0.0..=1.0).contains(&self.wetness),
            "Material wetness must be between zero and one"
        );
        ensure!(
            self.specular_aa.is_finite() && (0.0..=16.0).contains(&self.specular_aa),
            "Specular anti-aliasing strength must be between zero and 16"
        );
        ensure!(
            self.pattern
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 16.0),
            "Pattern coefficients must be finite and between -16 and 16"
        );
        Ok(())
    }

    pub(crate) fn value(&self, input: Input) -> Result<[f32; 4]> {
        Ok(match input {
            Input::Wetness => [self.wetness; 4],
            Input::SpecularAa => [self.specular_aa; 4],
            Input::Pattern(row) => *self
                .pattern
                .get(usize::from(row))
                .context("Pattern input row")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Input {
    Wetness,
    SpecularAa,
    Pattern(u8),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub constant: u8,
    pub input: Input,
}

/// Prepare private payload copies for preview and package emission. The pinned
/// graph and source files are immutable, and the source expression remains intact.
pub fn bind(
    graph: &Value,
    root: &Path,
    inputs: Option<&MaterialInputs>,
    bound: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    if graph.get("material_inputs").is_none() {
        ensure!(
            inputs.is_none(),
            "Imported graph has no editable material inputs. Reimport it"
        );
        return Ok(());
    }
    ensure!(
        graph["source"] == "marathon",
        "Unsupported material input source"
    );
    let defaults: MaterialInputs = serde_json::from_value(graph["material_inputs"].clone())?;
    let inputs = inputs.unwrap_or(&defaults);
    inputs.validate()?;
    for node in graph["nodes"].as_array().context("Material input nodes")? {
        let Some(bindings) = node.get("material_inputs") else {
            continue;
        };
        let bindings: Vec<Binding> = serde_json::from_value(bindings.clone())?;
        let name = node["symbol"].as_str().context("Material input symbol")?;
        let mut data = if let Some(data) = bound.remove(name) {
            data
        } else {
            let root = root.canonicalize()?;
            let file = root
                .join(node["file"].as_str().context("Material input file")?)
                .canonicalize()?;
            ensure!(file.starts_with(&root), "Material input file escapes graph");
            fs::read(file)?
        };
        let rows = super::Payload(data.clone()).array(0x2f8, 16, Some(0x80800090))?;
        let mut seen = BTreeSet::new();
        for binding in bindings {
            ensure!(
                seen.insert(binding.constant),
                "Duplicate material input constant"
            );
            let at = *rows
                .get(usize::from(binding.constant))
                .context("Material input constant index")?;
            let value = inputs.value(binding.input)?;
            for (lane, value) in value.into_iter().enumerate() {
                data[at + lane * 4..at + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bound.insert(name.to_owned(), data);
    }
    Ok(())
}
