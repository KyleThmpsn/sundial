//! Explicit MCC action, overlay and transition routes.
use crate::halo_reach::cache::Cache;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub mode: String,
    pub weapon_class: String,
    pub weapon_type: String,
    pub set: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Kind::is_action")]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<Destination>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Action,
    Overlay,
    Transition,
}

impl Kind {
    fn is_action(&self) -> bool {
        *self == Self::Action
    }

    fn label(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Overlay => "overlay",
            Self::Transition => "transition",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub mode: String,
    pub state: String,
}

impl Route {
    pub(super) fn clip<'a>(&self, graph: &'a Value) -> Result<&'a str> {
        ensure!(
            (self.kind == Kind::Transition) == self.destination.is_some(),
            "A transition route requires exactly one destination and other routes cannot have one"
        );
        let fields = [
            ("mode", &self.mode),
            ("weapon_class", &self.weapon_class),
            ("weapon_type", &self.weapon_type),
            ("set", &self.set),
            ("state", &self.state),
        ];
        let selected = graph["routes"]
            .as_array()
            .context("Source action routes")?
            .iter()
            .filter(|row| {
                row["kind"].as_str().unwrap_or("action") == self.kind.label()
                    && self.destination.as_ref().is_none_or(|destination| {
                        row["destination"]["mode"] == destination.mode
                            && row["destination"]["state"] == destination.state
                    })
                    && fields
                        .iter()
                        .all(|(key, value)| row[*key].as_str() == Some(value.as_str()))
            })
            .collect::<Vec<_>>();
        ensure!(
            selected.len() == 1,
            "Source action route is absent or ambiguous: {self:?}"
        );
        let row = selected[0];
        ensure!(
            row["graph_index"] == -1,
            "Source route uses an unsupported legacy graph index: {self:?}"
        );
        row["clip"]
            .as_str()
            .with_context(|| format!("Source action route has no local clip: {self:?}"))
    }
}

pub(super) fn read(cache: &Cache, graph: usize) -> Result<Value> {
    let clips = cache.block(graph + 0x94, 0x3c)?;
    let label = |row| cache.string_id(cache.u32(row)?);
    let mut routes = Vec::new();
    for mode in cache.block(graph + 0x104, 0x30)? {
        for class in cache.block(mode + 0xc, 0x38)? {
            for kind in cache.block(class + 8, 0x14)? {
                for set in cache.block(kind + 8, 0x48)? {
                    let mut entries = Vec::new();
                    for (offset, kind) in [(0xc, Kind::Action), (0x18, Kind::Overlay)] {
                        entries.extend(
                            cache
                                .block(set + offset, 0xc)?
                                .into_iter()
                                .map(|row| (row, row, kind)),
                        );
                    }
                    for transition in cache.block(set + 0x30, 0x10)? {
                        entries.extend(
                            cache
                                .block(transition + 4, 0xc)?
                                .into_iter()
                                .map(|row| (transition, row, Kind::Transition)),
                        );
                    }
                    for (action, target, route_kind) in entries {
                        let graph_index = cache.i16(target + 8)?;
                        let index = cache.i16(target + 0xa)?;
                        let clip = if graph_index == -1 {
                            usize::try_from(index)
                                .ok()
                                .and_then(|index| clips.get(index))
                                .map(|&row| label(row))
                                .transpose()?
                        } else {
                            None
                        };
                        let mut route = json!({"mode":label(mode)?,"weapon_class":label(class)?,
                            "weapon_type":label(kind)?,"set":label(set)?,"state":label(action)?,
                            "kind":route_kind,"graph_index":graph_index,"animation_index":index,"clip":clip});
                        if route_kind == Kind::Transition {
                            route["destination"] =
                                json!({"mode":label(target)?,"state":label(target + 4)?});
                        }
                        routes.push(route);
                    }
                }
            }
        }
    }
    Ok(json!(routes))
}
