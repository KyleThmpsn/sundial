//! Compact native readings retained for ingredient previews.
use serde::{Deserialize, Serialize};

use crate::sandbox_perk::action::{ActionSummary, SummaryLine};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailLine {
    pub text: String,
    pub kind: String,
    pub fields: Vec<String>,
    pub depth: usize,
    pub asset: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailSection {
    pub group: String,
    pub heading: String,
    pub lines: Vec<DetailLine>,
}

fn line(source: &SummaryLine) -> DetailLine {
    DetailLine {
        text: source.text.clone(),
        kind: source.kind_name.clone(),
        fields: source.detail.clone(),
        depth: source.depth,
        asset: source.asset,
    }
}

pub(super) fn sections(summary: &ActionSummary) -> Vec<DetailSection> {
    summary
        .groups
        .iter()
        .flat_map(|group| {
            [
                ("Starts When", &group.activation),
                ("Then", &group.effects),
                ("Ends When", &group.removal),
                ("Ready Again When", &group.rearm),
            ]
            .into_iter()
            .filter(|(_, lines)| !lines.is_empty())
            .map(|(heading, lines)| DetailSection {
                group: group.label.clone(),
                heading: heading.to_owned(),
                lines: lines.iter().map(line).collect(),
            })
        })
        .collect()
}
