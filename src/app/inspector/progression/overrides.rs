//! Family 5 override coverage and forced-value metadata.

use eframe::egui;

use crate::app::progression::CollectionStateSnapshot;
use crate::catalog::{Catalog, ProgressionContextKind, UnlockDefinition};

use crate::app::inspector::{metadata_field, progression_context_kind_label};

use super::{
    conditions::{
        decoded_condition_opcode, definition_has_undecoded_opcodes, direct_value_comparison,
    },
    definitions::flag_override_state_label,
    state::MetadataSelection,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum OverrideFilter {
    #[default]
    All,
    Unmapped,
    NoResolvedReaders,
    PartiallyDecoded,
}

impl OverrideFilter {
    pub(in crate::app) const ALL: [Self; 4] = [
        Self::All,
        Self::Unmapped,
        Self::NoResolvedReaders,
        Self::PartiallyDecoded,
    ];

    pub(in crate::app) const fn label(self) -> &'static str {
        match self {
            Self::All => "All Coverage",
            Self::Unmapped => "Not in Package Table",
            Self::NoResolvedReaders => "No Package References",
            Self::PartiallyDecoded => "Partially Decoded",
        }
    }
}

/// The Family 5 override the current save state holds for an override selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SavedOverride {
    Flag(u8),
    Value(i32),
}

impl SavedOverride {
    pub(super) fn current(
        selection: MetadataSelection,
        snapshot: &CollectionStateSnapshot,
    ) -> Option<Self> {
        match selection {
            MetadataSelection::FlagOverride(index) => {
                snapshot.flag_overrides.get(&index).copied().map(Self::Flag)
            }
            MetadataSelection::ValueOverride(index) => snapshot
                .value_overrides
                .get(&index)
                .copied()
                .map(Self::Value),
            MetadataSelection::FlagDefinition(_) | MetadataSelection::ValueDefinition(_) => None,
        }
    }
}

pub(super) fn draw_override_metadata(
    ui: &mut egui::Ui,
    index: usize,
    saved: SavedOverride,
    definition: &UnlockDefinition,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
) {
    let evaluated_flag = match saved {
        SavedOverride::Flag(_) => snapshot.evaluated_flag(index, catalog),
        SavedOverride::Value(_) => None,
    };
    let usage = override_usage_summary(index, saved, definition, evaluated_flag);
    let condition_program_decode = if definition.tested_by.is_empty() {
        "No condition programs"
    } else if definition_has_undecoded_opcodes(definition) {
        "Contains undecoded opcodes"
    } else {
        "All opcodes decoded"
    };
    egui::Grid::new("progression_override_metadata")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            match saved {
                SavedOverride::Flag(value) => {
                    metadata_field(
                        ui,
                        "Logical Flag Value",
                        flag_override_state_label(value),
                        false,
                    );
                    metadata_field(
                        ui,
                        "Settings Field",
                        "state.investment.family5_flag_overrides",
                        true,
                    );
                }
                SavedOverride::Value(value) => {
                    metadata_field(ui, "Value", value.to_string(), true);
                    metadata_field(
                        ui,
                        "Settings Field",
                        "state.investment.family5_value_overrides",
                        true,
                    );
                }
            }
            metadata_field(
                ui,
                "Condition Program Decode",
                condition_program_decode,
                false,
            );
            metadata_field(ui, "Known References", usage.readers, false);
            metadata_field(ui, "Reference Types", usage.reader_types, false);
            metadata_field(ui, "Condition Usage", usage.condition_usage, false);
            if let Some(impact) = usage.forced_impact {
                metadata_field(ui, "Result", impact, false);
            }
            if let Some(opcodes) = usage.undecoded_opcodes {
                metadata_field(ui, "Undecoded Opcodes", opcodes, true);
            }
        });
}

struct OverrideUsageSummary {
    readers: String,
    reader_types: String,
    condition_usage: String,
    forced_impact: Option<String>,
    undecoded_opcodes: Option<String>,
}

pub(in crate::app) fn override_filter_matches(
    filter: OverrideFilter,
    definition: Option<&UnlockDefinition>,
) -> bool {
    match filter {
        OverrideFilter::All => true,
        OverrideFilter::Unmapped => definition.is_none(),
        OverrideFilter::NoResolvedReaders => {
            definition.is_some_and(|definition| definition.tested_by.is_empty())
        }
        OverrideFilter::PartiallyDecoded => {
            definition.is_some_and(definition_has_undecoded_opcodes)
        }
    }
}

fn override_usage_summary(
    index: usize,
    saved: SavedOverride,
    definition: &UnlockDefinition,
    evaluated_flag: Option<bool>,
) -> OverrideUsageSummary {
    let mut reader_types = Vec::<(ProgressionContextKind, usize)>::new();
    let mut programs = Vec::<Vec<[u32; 2]>>::new();
    for context in &definition.tested_by {
        if let Some((_, count)) = reader_types
            .iter_mut()
            .find(|(kind, _)| *kind == context.kind)
        {
            *count += 1;
        } else {
            reader_types.push((context.kind, 1));
        }
        programs.extend(context.condition_programs.iter().cloned());
    }
    let program_count = programs.len();
    programs.sort();
    programs.dedup();

    let readers = if definition.tested_by.is_empty() {
        "No package reference found".to_owned()
    } else {
        format!(
            "{} exact package {}",
            definition.tested_by.len(),
            if definition.tested_by.len() == 1 {
                "relationship"
            } else {
                "relationships"
            }
        )
    };
    let reader_types = if reader_types.is_empty() {
        "None resolved".to_owned()
    } else {
        reader_types
            .into_iter()
            .map(|(kind, count)| format!("{} {count}", progression_context_kind_label(kind)))
            .collect::<Vec<_>>()
            .join(" · ")
    };

    let mut undecoded = programs
        .iter()
        .flat_map(|program| program.iter().map(|token| token[0]))
        .filter(|opcode| !decoded_condition_opcode(*opcode))
        .collect::<Vec<_>>();
    undecoded.sort_unstable();
    undecoded.dedup();
    let undecoded_opcodes = (!undecoded.is_empty()).then(|| {
        undecoded
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    });

    let (condition_usage, forced_impact) = match saved {
        SavedOverride::Flag(_) => {
            let direct = programs
                .iter()
                .filter(|program| program.as_slice() == [[1, index as u32]])
                .count();
            let negated = programs
                .iter()
                .filter(|program| {
                    program.len() == 2 && program[0] == [1, index as u32] && program[1][0] == 2
                })
                .count();
            let composite = programs.len().saturating_sub(direct + negated);
            let usage = format!(
                "{program_count} program {}, {} unique · {direct} direct · {negated} negated · {composite} composite/other",
                if program_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                },
                programs.len()
            );
            let impact = Some(evaluated_flag.map_or_else(
                || "Unresolved".to_owned(),
                |value| {
                    format!(
                        "Direct checks read {value}. Negated checks read {}.",
                        !value
                    )
                },
            ));
            (usage, impact)
        }
        SavedOverride::Value(value) => {
            let mut comparisons = programs
                .iter()
                .filter_map(|program| direct_value_comparison(program, index, value))
                .collect::<Vec<_>>();
            comparisons.sort_by(|left, right| left.0.cmp(&right.0));
            comparisons.dedup_by(|left, right| left.0 == right.0);
            let composite = programs.len().saturating_sub(comparisons.len());
            let mut labels = comparisons
                .iter()
                .map(|(label, _)| label.clone())
                .take(10)
                .collect::<Vec<_>>();
            if comparisons.len() > 10 {
                labels.push(format!("+{} more", comparisons.len() - 10));
            }
            let direct_text = if labels.is_empty() {
                "no standalone decoded comparison".to_owned()
            } else {
                format!("direct {}", labels.join(", "))
            };
            let usage = format!(
                "{program_count} program {}, {} unique · {direct_text} · {composite} composite/other",
                if program_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                },
                programs.len()
            );
            let passed = comparisons.iter().filter(|(_, result)| *result).count();
            let impact = (!comparisons.is_empty()).then(|| {
                format!(
                    "At {value}: {passed} decoded direct {} pass, {} fail",
                    if passed == 1 {
                        "comparison"
                    } else {
                        "comparisons"
                    },
                    comparisons.len() - passed
                )
            });
            (usage, impact)
        }
    };

    OverrideUsageSummary {
        readers,
        reader_types,
        condition_usage,
        forced_impact,
        undecoded_opcodes,
    }
}
