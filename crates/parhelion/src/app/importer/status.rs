use super::*;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
pub(super) struct Record {
    pub working: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Default, PartialEq, Eq, Clone, Copy, serde::Deserialize, serde::Serialize)]
pub(super) enum Filter {
    #[default]
    All,
    Working,
    NotTested,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::All, Filter::Working, Filter::NotTested];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "Any Status",
            Filter::Working => "Working",
            Filter::NotTested => "Not Tested",
        }
    }
}

pub(super) fn name(working: bool) -> &'static str {
    if working { "Working" } else { "Not Tested" }
}

pub(super) fn color(visuals: &egui::Visuals, working: bool) -> egui::Color32 {
    if working {
        style::success_color(visuals)
    } else {
        visuals.warn_fg_color
    }
}

pub(super) fn load() -> Result<BTreeMap<u32, Record>, String> {
    let path = data_root()?.join("importer-status.json");
    let local: BTreeMap<u32, Record> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.to_string()),
    }?;
    let mut records: BTreeMap<_, _> = service::known_weapons()
        .map(|(hash, name)| {
            (
                hash,
                Record {
                    working: true,
                    note: format!("{name}: working in the original library."),
                },
            )
        })
        .collect();
    records.extend(local);
    Ok(records)
}

pub(super) fn save(records: &BTreeMap<u32, Record>) -> Result<(), String> {
    let path = data_root()?.join("importer-status.json");
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| error.to_string())?;
    sundial::package_authoring::replace_authoring_file(&path, &bytes)
        .map_err(|error| error.to_string())
}
