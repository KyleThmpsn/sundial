use super::Program;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBindings {
    pub activity_id: String,
    /// Exact SDK snapshot identity returned by the target host.
    pub sdk_build_id: String,
    #[serde(default)]
    pub objects: Vec<ObjectBinding>,
    #[serde(default)]
    pub completion: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectBinding {
    pub source_index: u16,
    pub source_name: String,
    pub slot_id: String,
}

impl NativeBindings {
    pub(super) fn validate(&self, source: &Program) -> Result<()> {
        let identity = |value: &str| {
            !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        };
        ensure!(
            identity(&self.activity_id),
            "Invalid native activity identity"
        );
        ensure!(
            self.sdk_build_id
                .strip_prefix("sha256:")
                .is_some_and(|v| v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
            "Native SDK identity must be sha256:<64 lowercase hex>"
        );
        let (mut indices, mut names, mut slots) =
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for object in &self.objects {
            ensure!(
                identity(&object.source_name) && identity(&object.slot_id),
                "Invalid object binding identity"
            );
            ensure!(
                indices.insert(object.source_index)
                    && names.insert(&object.source_name)
                    && slots.insert(&object.slot_id),
                "Duplicate or aliased native object binding"
            );
            let mut matched = false;
            for named in
                source.bindings.named.iter().filter(|n| {
                    n.domain == "object_name" && n.index == u32::from(object.source_index)
                })
            {
                ensure!(
                    named.name.as_deref() == Some(&object.source_name),
                    "Stale source object name"
                );
                matched = true;
            }
            ensure!(matched, "Object binding has no retained source identity");
        }
        Ok(())
    }

    pub(super) fn object<'a>(&'a self, source: &Program, datum: u32) -> Result<&'a str> {
        let node = source.node(datum)?;
        ensure!(
            node.kind() == 9 && matches!(node.literal_type(), 78..=84) && node.value != u32::MAX,
            "Native object operations require a constant named object"
        );
        let matches: Vec<_> = source
            .bindings
            .named
            .iter()
            .filter(|n| n.expression == datum)
            .collect();
        ensure!(
            matches.len() == 1,
            "Native object argument needs one retained source binding"
        );
        let named = matches[0];
        ensure!(
            named.domain == "object_name"
                && named.value_type == node.literal_type()
                && named.value == node.value
                && named.index == node.value & 0xffff,
            "Native object argument differs from its source binding"
        );
        let binding = self
            .objects
            .iter()
            .find(|b| {
                u32::from(b.source_index) == named.index
                    && Some(&b.source_name) == named.name.as_ref()
            })
            .context("Source object has no explicit target binding")?;
        Ok(&binding.slot_id)
    }
}
