//! Source plugs that Shadowkeep has no counterpart for become private plugs on the socket where
//! the base keeps its own plug, when their controllers translate. The private plug carries the
//! source name, description and icon, so the socket shows the source perk and runs its
//! translated behavior instead of the base's.
use super::perks::{
    self,
    entity::{self, Groups},
    lifecycle, machine, modifier, presentation, table,
};
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

/// The edge of the square artwork a recipe embeds.
const ICON_EDGE: u32 = 96;

/// Intrinsic plugs (`67FBA961`) set the weapon's frame, which belongs to the base's runtime.
const INTRINSIC_CATEGORY: u64 = 0x67FB_A961;
/// Weapon perk plugs, as opposed to shaders (14), masterworks and trackers (13) and mods (37).
const PERK_ITEM_KIND: u64 = 15;
/// The empty catalyst plug's category (`72334481`).
const EMPTY_CATALYST_CATEGORY: u64 = 0x7233_4481;
/// Kill tracker plugs (`AFB3306E`), masterwork items of one category like a first-year catalyst.
const TRACKER_CATEGORY: u64 = 0xAFB3_306E;
/// The most choices a column offers freely. The rest only roll.
const REUSABLE_CHOICES: usize = 4;
/// The most choices a column holds, the rows one ownership mask covers.
const ROLL_POOL: usize = 64;

/// Both package sets, for attachments whose entity must be read from the source and written
/// into a Shadowkeep graph.
struct Packages<'a> {
    source: &'a mut Reader,
    native: &'a mut Reader,
    /// Where whole entities are converted, or none to leave them out.
    groups: Option<Groups<'a>>,
}

impl modifier::Tags for Packages<'_> {
    fn reference(&mut self, p: &Payload, field: usize) -> Result<u32> {
        self.source.ref64(p, field)
    }

    fn source(&mut self, tag: u32, class: u32) -> Result<Arc<Payload>> {
        self.source.tag(tag, Some(class))
    }

    fn native(&mut self, tag: u32) -> Result<Arc<Payload>> {
        self.native.tag(tag, None)
    }

    fn whole(&mut self, p: &Payload, at: usize) -> Result<modifier::Whole> {
        let groups = self
            .groups
            .context("whole entities are converted for a plug's first choices only")?;
        entity::convert(self.source, self.native, groups, p, at)
    }

    /// A status whose icon cannot be read keeps the generic buff icon.
    fn status_image(&mut self, name: u32) -> Option<String> {
        let png = presentation::status_icon(self.source, name)
            .ok()
            .flatten()?;
        Some(base64::engine::general_purpose::STANDARD.encode(png))
    }
}

/// Add a private plug for each socket fallback in `report` whose source plug translates. A perk
/// socket the source fills with nothing it can run is disabled, so the base's own perk does not
/// stand in for the source's. `lenders` lists stock weapons, those of the base's type first,
/// whose exotics can lend a base without a catalyst socket the type of theirs. Return what was
/// translated, disabled and kept.
pub fn apply(
    modern: &Path,
    native: &Path,
    report: &Value,
    output: &Path,
    recipe: &mut Value,
    lenders: &[u32],
) -> Result<Value> {
    let fallbacks = report["fallbacks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                usize::try_from(row["socket"].as_u64()?).ok()?,
                u32::try_from(row["source_default"].as_u64()?).ok()?,
                u32::try_from(row["retained_default"].as_u64()?).ok()?,
            ))
        })
        .collect::<Vec<_>>();
    let mut translated = Vec::new();
    let mut retained = Vec::new();
    let mut readers = None;
    let mut carriers = None;
    // Sockets whose default this step made private, and the source default of every fallback.
    let mut defaults = BTreeSet::new();
    let source_defaults = fallbacks
        .iter()
        .map(|(socket, plug, _)| (*socket, *plug))
        .collect::<BTreeMap<_, _>>();
    for (socket, plug, base_plug) in fallbacks {
        let (source, native_reader) = open_readers(&mut readers, modern, native, output)?;
        if let Some(reason) = kept_lane(recipe, socket, base_plug) {
            retained
                .push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),"reason":reason}));
            continue;
        }
        let lent = carriers.get_or_insert_with(|| base_carriers(native_reader, report));
        let groups = Some(Groups {
            modern,
            native,
            output,
            plug,
        });
        match private_plug(
            source,
            native_reader,
            (lent, groups),
            socket,
            0,
            plug,
            base_plug,
            false,
        ) {
            Ok((variant, omitted)) => {
                defaults.insert(socket);
                pin_column(recipe, socket, base_plug)?;
                let name = variant["name"].clone();
                push_variant(recipe, variant)?;
                translated.push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),
                    "name":name,"template_plug":format!("{base_plug:08X}"),"omitted":omitted,
                    "gameplay_verified":false}));
            }
            Err(error) => retained
                .push(json!({"socket":socket,"source_plug":format!("{plug:08X}"),
                "base_plug":format!("{base_plug:08X}"),"reason":format!("{error:#}")})),
        }
    }
    add_new_columns(report, recipe, &mut retained)?;
    {
        let (_, native_reader) = open_readers(&mut readers, modern, native, output)?;
        add_standard_lanes(report, recipe, native_reader, lenders, &mut retained)?;
    }
    // The plugs a column offers beyond its default that Shadowkeep lacks, a catalyst among them.
    // Each one that translates joins the column as a private choice after the stock ones.
    for row in report["fallbacks"].as_array().into_iter().flatten() {
        let Some(plugs) = row["unsupported_plugs"].as_array() else {
            continue;
        };
        let lane = row["socket"].as_u64().and_then(|v| usize::try_from(v).ok());
        if lane.is_none() && row["new_column"] != true {
            continue;
        }
        let mut socket = lane;
        let (source, native_reader) = open_readers(&mut readers, modern, native, output)?;
        let authored = lane.is_some_and(|lane| {
            !defaults.contains(&lane)
                && recipe["overrides"]["socket_plug_variants"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["socket_index"] == lane))
        });
        if authored {
            continue;
        }
        // A column added after the base's lanes takes its template from the lane it copies.
        let layout = row["layout_lane"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok());
        let (taken, found) = column_template(report, native_reader, lenders, layout.or(lane));
        if let Some(lane) = taken {
            socket = Some(lane);
        }
        let (column_type, template) = match found {
            Ok(found) => found,
            Err(error) => {
                retained.push(json!({"socket":socket,"source_socket":row["source_socket"],
                    "reason":format!("{error:#}")}));
                continue;
            }
        };
        for plug in plugs
            .iter()
            .filter_map(|plug| u32::try_from(plug.as_u64()?).ok())
        {
            if lane.is_some_and(|lane| source_defaults.get(&lane) == Some(&plug)) {
                continue;
            }
            let choice = socket.map_or(0, |socket| {
                recipe["overrides"]["socket_columns"][socket]["choices"]
                    .as_array()
                    .map_or(0, Vec::len)
            });
            // The column a private choice would open, before anything is written.
            let target = match socket {
                Some(socket) => socket,
                None => next_column(report, recipe),
            };
            if choice >= ROLL_POOL {
                retained.push(json!({"socket":target,"source_plug":format!("{plug:08X}"),
                    "reason":"the socket's roll pool is full"}));
                continue;
            }
            let lent = carriers.get_or_insert_with(|| base_carriers(native_reader, report));
            // A whole entity costs a particle conversion, so only the choices a column
            // offers freely take one.
            let groups = (choice < REUSABLE_CHOICES).then_some(Groups {
                modern,
                native,
                output,
                plug,
            });
            match private_plug(
                source,
                native_reader,
                (lent, groups),
                target,
                choice,
                plug,
                template,
                true,
            ) {
                Ok((variant, omitted)) => {
                    if socket.is_none() {
                        socket = Some(new_column(report, recipe, &column_type)?);
                    }
                    add_choice(recipe, target, template)?;
                    let name = variant["name"].clone();
                    push_variant(recipe, variant)?;
                    translated.push(json!({"socket":target,"choice":choice,
                        "source_plug":format!("{plug:08X}"),"name":name,
                        "template_plug":format!("{template:08X}"),"omitted":omitted,
                        "gameplay_verified":false}));
                }
                Err(error) => retained.push(json!({"socket":socket,
                    "source_socket":row["source_socket"],
                    "source_plug":format!("{plug:08X}"),"reason":format!("{error:#}")})),
            }
        }
    }
    let mut disabled = disable_unfilled_perks(report, recipe)?;
    disabled.extend(close_base_catalysts(report, recipe)?);
    let result = json!({"translated":translated,"retained":retained,"disabled":disabled});
    std::fs::create_dir_all(output)?;
    write_json(&output.join("private-perks.json"), &result)?;
    Ok(result)
}

/// Why the default lane `socket` keeps what it holds instead of taking a private plug: the
/// socket already has an authored private plug, or the mapping already gave the lane another
/// source plug in place of `base_plug`.
fn kept_lane(recipe: &Value, socket: usize, base_plug: u32) -> Option<&'static str> {
    let authored = recipe["overrides"]["socket_plug_variants"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["socket_index"] == socket));
    if authored {
        return Some("the socket already has an authored private plug");
    }
    let occupied = recipe["overrides"]["socket_columns"][socket]["choices"]
        .as_array()
        .is_some_and(|choices| choices.first() != Some(&json!(format!("0x{base_plug:08X}"))));
    occupied.then_some("the lane already holds another source plug")
}

/// The source and native readers, opened on first use.
fn open_readers<'a>(
    readers: &'a mut Option<(Reader, Reader)>,
    modern: &Path,
    native: &Path,
    output: &Path,
) -> Result<&'a mut (Reader, Reader)> {
    if readers.is_none() {
        *readers = Some((
            Reader::discovery(modern, &output.join("source"), true)?,
            Reader::discovery(native, &output.join("native"), false)?,
        ));
    }
    readers.as_mut().context("discovery readers")
}

/// Appends a private plug variant to the recipe's socket plug variants.
fn push_variant(recipe: &mut Value, variant: Value) -> Result<()> {
    let variants = &mut recipe["overrides"]["socket_plug_variants"];
    if !variants.is_array() {
        *variants = json!([]);
    }
    variants
        .as_array_mut()
        .context("socket plug variants")?
        .push(variant);
    Ok(())
}

/// Weapon mods and the first-year Masterwork Upgrade plug are items of this kind.
const MOD_ITEM_KIND: u64 = 37;

/// A socket that offers kill trackers, on a source weapon or a base.
fn tracker_lane(socket: &Value) -> bool {
    socket["choices"].as_array().is_some_and(|choices| {
        choices
            .iter()
            .any(|choice| choice["category"].as_u64() == Some(TRACKER_CATEGORY))
    })
}

/// A weapon mod socket: every plug it offers is a mod.
pub(super) fn mod_lane(socket: &Value) -> bool {
    socket["socket_type"] != u16::MAX
        && socket["choices"].as_array().is_some_and(|choices| {
            !choices.is_empty()
                && choices
                    .iter()
                    .all(|choice| choice["item_kind"].as_u64() == Some(MOD_ITEM_KIND))
        })
}

/// A base lane that holds masterwork tiers: only masterwork items (13), and neither a kill
/// tracker nor a catalyst socket. A tier socket narrowed to one stat, which `masterwork_lane`
/// does not tell apart, is one. So is an ornament socket, whose source plugs name art and
/// never join a column.
pub(super) fn tier_lane(socket: &Value) -> bool {
    socket["socket_type"] != u16::MAX
        && !tracker_lane(socket)
        && stock_catalysts(socket).is_none()
        && socket["choices"].as_array().is_some_and(|choices| {
            !choices.is_empty()
                && choices
                    .iter()
                    .all(|choice| choice["item_kind"].as_u64() == Some(13))
        })
}

/// The first-year socket that starts on Masterwork Upgrade, a mod item, and offers masterwork
/// plugs beside it. Weapons of the later layout have a tier socket and a mod socket instead.
fn upgrade_lane(socket: &Value) -> bool {
    socket["socket_type"] != u16::MAX
        && socket["choices"].as_array().is_some_and(|choices| {
            let kind = |choice: &Value| choice["item_kind"].as_u64();
            choices
                .iter()
                .any(|choice| choice["default"] == true && kind(choice) == Some(MOD_ITEM_KIND))
                && choices.iter().any(|choice| kind(choice) == Some(13))
        })
}

type LanePredicate = fn(&Value) -> bool;

/// The first stock weapon that lends a socket of the requested kind.
fn lender_lane(
    native: &mut Reader,
    lenders: &[u32],
    is_lane: LanePredicate,
) -> Option<(u32, Value)> {
    for &lender in lenders {
        let Ok(sockets) = crate::tiger::shadowkeep::sockets(native, lender) else {
            continue;
        };
        if let Some(lane) = sockets["sockets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|lane| is_lane(lane))
        {
            return Some((lender, lane.clone()));
        }
    }
    None
}

/// Close first-year upgrade sockets replaced by separate masterwork or mod sockets.
fn close_upgrade_lanes(base: &[Value], taken: &BTreeSet<usize>, recipe: &mut Value) {
    for (lane, socket) in base.iter().enumerate() {
        if upgrade_lane(socket) && !taken.contains(&lane) {
            let columns = &mut recipe["overrides"]["socket_columns"];
            if let Some(columns) = columns.as_array_mut() {
                if columns.len() <= lane {
                    columns.resize(lane + 1, Value::Null);
                }
                columns[lane] = json!({"choices":[],"socket_type":u16::MAX});
            }
        }
    }
}

/// Give the import the sockets of the later weapon layout that its base lacks: the kill
/// tracker, the masterwork tier socket and the mod socket. A socket type whitelists plug
/// categories and fits any weapon, so each one comes from the first stock weapon that has it,
/// the base's own type first, as a new column with that weapon's socket type and stock plugs.
/// A first-year base's Masterwork Upgrade socket is closed once the tier or mod socket stands
/// in for it. An exotic source keeps its masterwork to the catalyst rules.
///
/// A base socket of the lender's socket type is that socket narrowed, as a tier socket fixed
/// to one stat is. It takes the lender's plugs in place, so the import has one such socket.
fn add_standard_lanes(
    report: &Value,
    recipe: &mut Value,
    native: &mut Reader,
    lenders: &[u32],
    retained: &mut Vec<Value>,
) -> Result<()> {
    let empty = Vec::new();
    let base = report["native_sockets"].as_array().unwrap_or(&empty);
    let source = report["source"]["sockets"].as_array().unwrap_or(&empty);
    let exotic = report["mapped"]["rarity"] == "exotic";
    let wanted: [(&str, LanePredicate, bool); 3] = [
        (
            "kill tracker",
            tracker_lane,
            source.iter().any(tracker_lane),
        ),
        ("masterwork", masterwork_lane, !exotic),
        ("mod", mod_lane, true),
    ];
    let mut replaced = false;
    // Base lanes that took a lender's plugs in place.
    let mut taken = BTreeSet::new();
    for (name, is_lane, wanted) in wanted {
        if !wanted || base.iter().any(is_lane) {
            continue;
        }
        let Some((lender, lane)) = lender_lane(native, lenders, is_lane) else {
            retained.push(json!({"reason":format!("no stock weapon lends a {name} socket")}));
            continue;
        };
        let narrowed = base
            .iter()
            .position(|socket| socket["socket_type"] == lane["socket_type"]);
        let column = match narrowed {
            Some(own) => {
                let columns = &mut recipe["overrides"]["socket_columns"];
                if !columns.is_array() {
                    *columns = json!([]);
                }
                let columns = columns.as_array_mut().context("socket columns")?;
                if columns.len() <= own {
                    columns.resize(own + 1, Value::Null);
                }
                columns[own] = json!({"choices":[]});
                taken.insert(own);
                own
            }
            None => new_column(report, recipe, &lane["socket_type"])?,
        };
        // The lender's starting plug leads, as it does on the lender.
        let mut plugs = lane["choices"].as_array().cloned().unwrap_or_default();
        plugs.sort_by_key(|choice| choice["default"] != true);
        let hashes = plugs
            .iter()
            .filter_map(|choice| u32::try_from(choice["hash"].as_u64()?).ok())
            .collect::<Vec<_>>();
        let set = lane["reusable_set"]
            .as_u64()
            .and_then(|set| u16::try_from(set).ok());
        if let (Some(set), Some(&first)) = (set, hashes.first()) {
            // The lender's socket offers its plugs through a stock reusable plug set. The new
            // socket names the same set and starts on the same plug, so it offers every plug
            // of the set however many there are.
            add_choice(recipe, column, first)?;
            recipe["overrides"]["socket_columns"][column]["reusable_plug_set_index"] = json!(set);
        } else {
            // A socket that lists its own plugs is copied plug by plug, 64 at most.
            for &plug in hashes.iter().take(ROLL_POOL) {
                add_choice(recipe, column, plug)?;
            }
        }
        // These sockets offer every plug, as they do on the lender. None of them rolls.
        if let Some(added) = recipe["overrides"]["socket_columns"][column].as_object_mut() {
            added.remove("reusable_choices");
            added.remove("randomized_selection_program");
        }
        let reason = if narrowed.is_some() {
            format!("the base's {name} socket offers one plug, so it takes a stock weapon's plugs")
        } else {
            format!("the base has no {name} socket, so the import takes a stock weapon's")
        };
        retained.push(json!({"socket":column,"lender":format!("{lender:08X}"),"reason":reason}));
        replaced |= name != "kill tracker";
    }
    if replaced {
        close_upgrade_lanes(base, &taken, recipe);
    }
    Ok(())
}

/// A stock plug with no lane of its own category and no free lane takes a new column.
fn add_new_columns(report: &Value, recipe: &mut Value, retained: &mut Vec<Value>) -> Result<()> {
    for row in report["fallbacks"].as_array().into_iter().flatten() {
        let (true, Some(plug)) = (
            row["new_column"] == true,
            row["placed_plug"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok()),
        ) else {
            continue;
        };
        match perk_layout(report) {
            Some(layout) => {
                let socket = new_column(
                    report,
                    recipe,
                    &report["native_sockets"][layout]["socket_type"],
                )?;
                add_choice(recipe, socket, plug)?;
            }
            None => retained.push(json!({"source_plug":format!("{plug:08X}"),
                "reason":"the base has no perk lane to lend a new column its layout"})),
        }
    }
    Ok(())
}

/// The column type and template plug for the unsupported plugs of a source socket on `lane`,
/// and the base lane a catalyst takes over, if it takes one.
///
/// A source socket with no lane is a catalyst socket. On a base with a catalyst socket of its
/// own the catalyst goes there, as a copy of one of its stock catalysts. On a base with a
/// masterwork tier socket it takes that socket in place of the tiers, as a catalyst is an
/// exotic's masterwork. On any other base it takes a new column that is only added once one of
/// its plugs translates: the catalyst socket of a stock exotic, or where no lender has one, the
/// layout of one of the base's perk lanes.
fn column_template(
    report: &Value,
    native_reader: &mut Reader,
    lenders: &[u32],
    lane: Option<usize>,
) -> (Option<usize>, Result<(Value, u32)>) {
    let catalyst = match lane {
        Some(_) => None,
        None => match catalyst_layout(report, native_reader) {
            Some(found) => Some(found),
            None => masterwork_layout(report, native_reader),
        },
    };
    let borrowed = match (lane, catalyst) {
        (None, None) => kin_catalyst(native_reader, lenders),
        _ => None,
    };
    let lane_type = |lane: usize| report["native_sockets"][lane]["socket_type"].clone();
    let found = match (lane, catalyst, borrowed) {
        (Some(lane), ..) => template_plug(native_reader, &report["native_sockets"][lane], false)
            .map(|template| (lane_type(lane), template)),
        (None, Some((lane, template)), _) => Ok((lane_type(lane), template)),
        (None, None, Some(found)) => Ok(found),
        (None, None, None) => carrying_layout(report, native_reader)
            .map(|(lane, template)| (lane_type(lane), template)),
    };
    (catalyst.map(|(lane, _)| lane), found)
}

/// Disable each non-intrinsic perk socket that carries no source plug, translated or native.
fn disable_unfilled_perks(report: &Value, recipe: &mut Value) -> Result<Vec<Value>> {
    let mut disabled = Vec::new();
    for (index, socket) in report["native_sockets"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(default) = socket["choices"]
            .as_array()
            .and_then(|choices| choices.iter().find(|choice| choice["default"] == true))
        else {
            continue;
        };
        if socket["socket_type"] == u16::MAX
            || default["item_kind"].as_u64() != Some(PERK_ITEM_KIND)
            || default["category"].as_u64() == Some(INTRINSIC_CATEGORY)
        {
            continue;
        }
        let private = recipe["overrides"]["socket_plug_variants"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["socket_index"] == index));
        let column = &recipe["overrides"]["socket_columns"][index];
        let filled = column["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice != &json!(format!("0x{:08X}", default["hash"].as_u64().unwrap_or(0)))
            })
        });
        let source_default = report["mapped"]["socket_columns"][index]["choices"]
            .as_array()
            .is_some_and(|choices| !choices.is_empty());
        let fallback = report["fallbacks"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["socket"] == index && row.get("retained_default").is_some())
        });
        if private || (source_default && !fallback) || filled {
            continue;
        }
        let columns = &mut recipe["overrides"]["socket_columns"];
        if !columns.is_array() {
            *columns = json!([]);
        }
        let columns = columns.as_array_mut().context("socket columns")?;
        if columns.len() <= index {
            columns.resize(index + 1, Value::Null);
        }
        columns[index] = json!({"choices":[],"socket_type":u16::MAX});
        disabled.push(json!({"socket":index,"base_plug":format!("{:08X}", default["hash"].as_u64().unwrap_or(0))}));
    }
    Ok(disabled)
}

/// The base's last perk lane other than the frame, whose socket type and plugs a new column
/// borrows.
fn perk_layout(report: &Value) -> Option<usize> {
    let lanes = report["native_sockets"].as_array()?;
    (0..lanes.len()).rev().find(|&lane| perk_lane(&lanes[lane]))
}

/// Whether a base lane offers perks other than the frame.
fn perk_lane(socket: &Value) -> bool {
    socket["socket_type"] != u16::MAX
        && socket["choices"]
            .as_array()
            .and_then(|choices| choices.iter().find(|choice| choice["default"] == true))
            .is_some_and(|default| {
                default["item_kind"].as_u64() == Some(PERK_ITEM_KIND)
                    && default["category"].as_u64() != Some(INTRINSIC_CATEGORY)
            })
}

/// The base's last perk lane other than the frame that has a stock plug with a finished perk,
/// and that plug. A new column of translated plugs borrows the lane's socket type and the
/// plug's item layout.
fn carrying_layout(report: &Value, native: &mut Reader) -> Result<(usize, u32)> {
    let lanes = report["native_sockets"]
        .as_array()
        .context("base sockets")?;
    for lane in (0..lanes.len()).rev() {
        if perk_lane(&lanes[lane])
            && let Ok(template) = template_plug(native, &lanes[lane], true)
        {
            return Ok((lane, template));
        }
    }
    anyhow::bail!(
        "the base has no perk lane whose stock plug has a finished perk to lend a new column"
    )
}

/// The stock catalysts of a lane, when it is an exotic's catalyst socket. Shadowkeep has two
/// forms, and each exotic has a socket type of its own. The later form offers the empty
/// catalyst plug, a masterwork item (13), beside catalysts that are mod items (37) of one
/// category. The first-year form offers only masterwork items of one category: a placeholder,
/// the catalyst and its upgrade. A kill tracker socket has that shape too and is told apart by
/// its category. A first-year legendary's masterwork socket, which offers masterwork items of
/// two categories beside an empty mod plug, is not a catalyst socket. The rule matches every
/// catalyst socket of the stock weapons and no other socket of theirs.
fn stock_catalysts(socket: &Value) -> Option<Vec<Value>> {
    if socket["socket_type"] == u16::MAX {
        return None;
    }
    let kind = |choice: &Value| choice["item_kind"].as_u64();
    let category = |choice: &Value| choice["category"].as_u64();
    let (empty, catalysts): (Vec<&Value>, Vec<&Value>) = socket["choices"]
        .as_array()?
        .iter()
        .partition(|choice| category(choice) == Some(EMPTY_CATALYST_CATEGORY));
    let shared = category(catalysts.first()?)?;
    if catalysts
        .iter()
        .any(|choice| category(choice) != Some(shared))
    {
        return None;
    }
    let later = !empty.is_empty()
        && empty.iter().all(|choice| kind(choice) == Some(13))
        && catalysts.iter().all(|choice| kind(choice) == Some(37));
    let first_year = empty.is_empty()
        && catalysts.len() > 1
        && shared != TRACKER_CATEGORY
        && catalysts.iter().all(|choice| kind(choice) == Some(13))
        && catalysts.iter().any(|choice| choice["default"] == true);
    (later || first_year).then(|| catalysts.into_iter().cloned().collect())
}

/// Whether a catalyst socket is of the later form, which starts on the empty catalyst plug.
fn starts_empty(socket: &Value) -> bool {
    socket["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|choice| choice["category"].as_u64() == Some(EMPTY_CATALYST_CATEGORY))
}

/// The base's catalyst socket and the stock catalyst a translated one copies for its item
/// layout.
fn catalyst_layout(report: &Value, native: &mut Reader) -> Option<(usize, u32)> {
    let lanes = report["native_sockets"].as_array()?;
    for (lane, socket) in lanes.iter().enumerate() {
        let Some(catalysts) = stock_catalysts(socket) else {
            continue;
        };
        let catalysts = json!({ "choices": catalysts });
        if let Ok(template) = template_plug(native, &catalysts, false) {
            return Some((lane, template));
        }
    }
    None
}

/// The base's masterwork tier socket and the tier plug a translated catalyst copies for its
/// item layout: the socket's starting plug, or its first where it starts on a rolled one. The
/// game draws that socket among the weapon mods on every weapon that has it, where a stock
/// exotic's catalyst socket carries a condition on one of that exotic's account flags.
fn masterwork_layout(report: &Value, native: &mut Reader) -> Option<(usize, u32)> {
    let lanes = report["native_sockets"].as_array()?;
    for (lane, socket) in lanes.iter().enumerate() {
        if !masterwork_lane(socket) {
            continue;
        }
        let choices = socket["choices"].as_array()?;
        let start = choices
            .iter()
            .find(|choice| choice["default"] == true)
            .or(choices.first())?;
        if let Ok(template) = template_plug(native, &json!({ "choices": [start] }), false) {
            return Some((lane, template));
        }
    }
    None
}

/// Whether a lane is a masterwork tier socket: only masterwork items (13), of three or more
/// categories, one for each stat its tiers can raise. Ornament sockets hold items of that kind
/// in two categories, and kill tracker and first-year catalyst sockets in one. The rule
/// matches the tier socket of every stock weapon that offers more than one stat and no other
/// socket of theirs. A tier socket narrowed to one stat is not told apart.
fn masterwork_lane(socket: &Value) -> bool {
    if socket["socket_type"] == u16::MAX {
        return false;
    }
    let Some(choices) = socket["choices"].as_array() else {
        return false;
    };
    let categories = choices
        .iter()
        .filter_map(|choice| choice["category"].as_u64())
        .collect::<BTreeSet<_>>();
    categories.len() > 2
        && choices
            .iter()
            .all(|choice| choice["item_kind"].as_u64() == Some(13))
}

/// A catalyst socket's type and one of its stock catalysts, from the stock exotics among
/// `lenders`. Each exotic's catalyst socket has a type of its own, whose whitelist names that
/// exotic's catalyst category, so a copy of its catalyst fits it on any weapon. The first
/// socket of the later form wins, since it starts empty and holds one catalyst. A first-year
/// socket, whose catalyst is a placeholder and an upgrade, is only taken when no lender has
/// the later form. The search keeps its reads in memory and records no socket report.
fn kin_catalyst(native: &mut Reader, lenders: &[u32]) -> Option<(Value, u32)> {
    let mut scan = native.shared_discovery();
    let mut first_year = None;
    for &weapon in lenders {
        // Only exotics, tier 5, have a catalyst socket.
        let exotic = crate::tiger::shadowkeep::item_tag(&mut scan, weapon)
            .and_then(|tag| scan.tag(tag, Some(0x8080_7BEA)))
            .and_then(|item| item.u8(0xBA))
            .is_ok_and(|tier| tier == 5);
        if !exotic {
            continue;
        }
        let Ok(sockets) = crate::tiger::shadowkeep::socket_report(&mut scan, weapon) else {
            continue;
        };
        for socket in sockets["sockets"].as_array().into_iter().flatten() {
            let Some(catalysts) = stock_catalysts(socket) else {
                continue;
            };
            if first_year.is_some() && !starts_empty(socket) {
                continue;
            }
            let Ok(template) = template_plug(native, &json!({ "choices": catalysts }), false)
            else {
                continue;
            };
            let found = (socket["socket_type"].clone(), template);
            if starts_empty(socket) {
                return Some(found);
            }
            first_year = Some(found);
        }
    }
    first_year
}

/// Close each catalyst socket of the base that the recipe leaves to the base, unless the
/// source weapon offers one of its catalysts too. Its catalysts upgrade the base weapon, not
/// the source one. Returns the lanes closed.
fn close_base_catalysts(report: &Value, recipe: &mut Value) -> Result<Vec<Value>> {
    let mut closed = Vec::new();
    // Every plug any source socket offers.
    let offered = report["source"]["sockets"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|socket| socket["choices"].as_array().into_iter().flatten())
        .map(|choice| choice["hash"].as_u64())
        .collect::<BTreeSet<_>>();
    for (lane, socket) in report["native_sockets"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(catalysts) = stock_catalysts(socket) else {
            continue;
        };
        // A base that is the source weapon's own Shadowkeep version offers the same catalyst.
        if catalysts
            .iter()
            .any(|catalyst| offered.contains(&catalyst["hash"].as_u64()))
        {
            continue;
        }
        let columns = &mut recipe["overrides"]["socket_columns"];
        if !columns.is_array() {
            *columns = json!([]);
        }
        let columns = columns.as_array_mut().context("socket columns")?;
        if columns.len() <= lane {
            columns.resize(lane + 1, Value::Null);
        }
        if columns[lane].is_null() {
            columns[lane] = json!({"choices":[],"socket_type":u16::MAX});
            closed.push(
                json!({"socket":lane,"reason":"the base's catalysts are not the source weapon's"}),
            );
        }
    }
    Ok(closed)
}

/// The index a new column would take: after the base's lanes and every column so far.
fn next_column(report: &Value, recipe: &Value) -> usize {
    let lanes = report["native_sockets"].as_array().map_or(0, Vec::len);
    let columns = recipe["overrides"]["socket_columns"]
        .as_array()
        .map_or(0, Vec::len);
    lanes.max(columns)
}

/// Add an empty column of `socket_type` after the base's lanes. Returns the new column.
fn new_column(report: &Value, recipe: &mut Value, socket_type: &Value) -> Result<usize> {
    let index = next_column(report, recipe);
    let columns = &mut recipe["overrides"]["socket_columns"];
    if !columns.is_array() {
        *columns = json!([]);
    }
    let columns = columns.as_array_mut().context("socket columns")?;
    columns.resize(index + 1, Value::Null);
    columns[index] = json!({"choices":[],"socket_type":socket_type});
    Ok(index)
}

/// Every finished perk of the base's stock perk plugs, defaults first and each once. A private
/// plug's programs each take the runtime of one finished perk, and a plug whose own template
/// has too few borrows the rest from here, as a translated plug only needs the rows.
fn base_carriers(native: &mut Reader, report: &Value) -> Vec<u16> {
    let mut carriers = Vec::new();
    let sockets = report["native_sockets"].as_array();
    for default in [true, false] {
        for choice in sockets
            .into_iter()
            .flatten()
            .flat_map(|socket| socket["choices"].as_array().into_iter().flatten())
        {
            if choice["item_kind"].as_u64() != Some(PERK_ITEM_KIND)
                || (choice["default"] == true) != default
            {
                continue;
            }
            let Some(hash) = choice["hash"].as_u64().and_then(|v| u32::try_from(v).ok()) else {
                continue;
            };
            let Ok(tag) = crate::tiger::shadowkeep::item_tag(native, hash) else {
                continue;
            };
            let Ok(item) = native.tag(tag, Some(0x8080_7BEA)) else {
                continue;
            };
            for perk in sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
                .unwrap_or_default()
            {
                if !carriers.contains(&perk) {
                    carriers.push(perk);
                }
            }
        }
    }
    carriers
}

/// The stock plug of a lane with the most finished perks, which a private choice of that lane
/// copies for its item layout and whose perks carry the translated programs first. Unless
/// `strict`, a lane whose plugs hold no finished perk lends its default plug's layout, and
/// the programs ride finished perks borrowed from the rest of the base.
fn template_plug(native: &mut Reader, socket: &Value, strict: bool) -> Result<u32> {
    let mut best = None;
    let mut layout = None;
    for choice in socket["choices"].as_array().into_iter().flatten() {
        let Some(hash) = choice["hash"].as_u64().and_then(|v| u32::try_from(v).ok()) else {
            continue;
        };
        let Ok(tag) = crate::tiger::shadowkeep::item_tag(native, hash) else {
            continue;
        };
        let item = native.tag(tag, Some(0x8080_7BEA))?;
        if layout.is_none() || choice["default"] == true {
            layout = Some(hash);
        }
        let mut perks = sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
            .map_err(anyhow::Error::msg)?;
        perks.dedup();
        if best.is_none_or(|(count, _)| perks.len() > count) && !perks.is_empty() {
            best = Some((perks.len(), hash));
        }
    }
    best.map(|(_, hash)| hash)
        .or(layout.filter(|_| !strict))
        .context("the lane has no stock plug with a finished perk to carry a translated one")
}

/// Append a private choice to a lane's column, which names `template` as every private choice
/// of a lane does. Choices past the freely offered ones only roll.
fn add_choice(recipe: &mut Value, socket: usize, template: u32) -> Result<()> {
    let columns = &mut recipe["overrides"]["socket_columns"];
    if !columns.is_array() {
        *columns = json!([]);
    }
    let columns = columns.as_array_mut().context("socket columns")?;
    if columns.len() <= socket {
        columns.resize(socket + 1, Value::Null);
    }
    let column = &mut columns[socket];
    if !column["choices"].is_array() {
        column["choices"] = json!([]);
    }
    let choices = column["choices"].as_array_mut().context("column choices")?;
    choices.push(json!(format!("0x{template:08X}")));
    let count = choices.len();
    if count > REUSABLE_CHOICES && column.get("reusable_choices").is_none() {
        column["reusable_choices"] = json!(REUSABLE_CHOICES);
        column["randomized_selection_program"] = json!([{"opcode":11,"operand":1}]);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn private_plug(
    source: &mut Reader,
    native: &mut Reader,
    (carriers, groups): (&[u16], Option<Groups<'_>>),
    socket: usize,
    choice: usize,
    plug: u32,
    base_plug: u32,
    offered: bool,
) -> Result<(Value, Vec<Value>)> {
    let assigned = perks::controllers(source, plug)?;
    let presentation = presentation::read(source, plug)?;
    // An enhanced variant repeats its plain perk, which the column already offers.
    anyhow::ensure!(
        !(offered
            && presentation.report["item_type"]
                .as_str()
                .is_some_and(|kind| kind.contains("Enhanced"))),
        "enhanced plugs are left out"
    );
    // Shadowkeep's label registry lets a label filter refuse a name Shadowkeep lacks. Without
    // it such a name would reach the package build, which stops on it.
    let registry = Some(
        sundial::investment::discovery::labels::Registry::read(
            &native.tag(table::NATIVE_LABELS, None)?.0,
        )
        .map_err(anyhow::Error::msg)?,
    );
    // The private plug clones the base's plug for its item layout. Each program replaces the
    // runtime of one finished perk, a different one for each program: the plug's own first,
    // then ones borrowed from the rest of the base.
    let tag = crate::tiger::shadowkeep::item_tag(native, base_plug)?;
    let item = native.tag(tag, Some(0x8080_7BEA))?;
    let mut templates = sundial::package_authoring::native_weapon::base_sandbox_perks(&item.0)
        .map_err(anyhow::Error::msg)?;
    templates.dedup();
    for perk in carriers {
        if !templates.contains(perk) {
            templates.push(*perk);
        }
    }
    let capacity = templates.len().min(machine::PROGRAM_LIMIT);
    anyhow::ensure!(
        capacity != 0,
        "the base has no finished perk to carry a translated program"
    );
    // Each controller takes the general translation of its states. One it refuses may still
    // carry a named property for the weapon's lifetime.
    let mut programs = Vec::new();
    let mut omitted = Vec::new();
    let mut packages = Packages {
        source: &mut *source,
        native: &mut *native,
        groups,
    };
    for assignment in &assigned {
        let source = std::slice::from_ref(assignment);
        let general = machine::translate(
            source,
            &presentation.name,
            registry.as_ref(),
            Some(&mut packages),
        );
        if !general.programs.is_empty() {
            // A controller's programs work together, so they are carried whole or not at all.
            if programs.len() + general.programs.len() > capacity {
                omitted.push(json!({"controller":format!("{:08X}", assignment.action_tag),
                    "reason":format!("its {} programs do not fit beside the {} the plug already carries",
                        general.programs.len(), programs.len())}));
                continue;
            }
            programs.extend(general.programs);
            omitted.extend(general.omitted);
            continue;
        }
        if programs.len() >= capacity {
            continue;
        }
        let lifetime = lifecycle::translate(source, &presentation.name);
        omitted.extend(if lifetime.programs.is_empty() {
            general.omitted
        } else {
            lifetime.omitted
        });
        programs.extend(lifetime.programs);
    }
    anyhow::ensure!(
        !programs.is_empty(),
        "no source controller translates: {}",
        serde_json::to_string(&omitted)?
    );
    let mut sandbox_perks = Vec::new();
    let mut projectile = None;
    for (program, template) in programs.iter().zip(&templates) {
        projectile = projectile.or_else(|| program.projectile_dependency());
        sandbox_perks.push(json!({"source_perk_index":template,
            "program":serde_json::to_value(program)?,"runtime_values":[],"action_float_values":[]}));
    }
    let mut variant = json!({"socket_index":socket,"choice_index":choice,
        "source_plug_hash":format!("0x{base_plug:08X}"),"name":presentation.name,
        "description":presentation.description,"replace_effects":true,
        "sandbox_perks":sandbox_perks});
    if let Some(dependency) = projectile {
        variant["projectile"] = serde_json::to_value(dependency)?;
    }
    if let Some(icon) = presentation.icon_png.as_deref().and_then(icon) {
        variant["icon"] =
            json!({"source":"image","name":presentation.name,"image":{"png_base64":icon}});
    }
    Ok((variant, omitted))
}

/// A square source icon at the recipe's artwork size, as base64 PNG.
fn icon(png: &[u8]) -> Option<String> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    if image.width() != image.height() || image.width() == 0 {
        return None;
    }
    let image = if image.width() == ICON_EDGE {
        image
    } else {
        image::imageops::resize(
            &image,
            ICON_EDGE,
            ICON_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}

/// Keep the base's plug as the socket's choice, so the private plug replaces exactly it.
fn pin_column(recipe: &mut Value, socket: usize, base_plug: u32) -> Result<()> {
    let columns = &mut recipe["overrides"]["socket_columns"];
    if !columns.is_array() {
        *columns = json!([]);
    }
    let columns = columns.as_array_mut().context("socket columns")?;
    if columns.len() <= socket {
        columns.resize(socket + 1, Value::Null);
    }
    if columns[socket].is_null() {
        columns[socket] = json!({"choices":[format!("0x{base_plug:08X}")]});
    }
    Ok(())
}
