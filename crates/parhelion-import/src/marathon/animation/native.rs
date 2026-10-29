//! Native layouts that carry the translated source controller.
//!
//! The native carrier supplies the engine classes and validated allocation layouts: the
//! first-person entity, its skeleton and control definitions, the lookup and pose controller
//! owners and their shared blend and look tables. Translated source clips and descriptors are
//! appended to the carrier's bank, whose own rows keep their ordinals, because the look table,
//! the pose layers and the actions the source does not translate address them directly. Each
//! translated action takes a private state node. Every other action keeps the carrier's state,
//! dictionary and supplemental poses.
use super::*;
use crate::tiger::animation::{self as native_animation, pose::Pose};
use std::sync::Arc;

pub(super) struct Carrier {
    pub runtime: u32,
    pub attachment: u32,
    pub entity: u32,
    pub lookup: (u32, Arc<Payload>),
    pub bank: (u32, Arc<Payload>),
    pub parameters: (u32, Arc<Payload>),
    pub states: (u32, Arc<Payload>),
    pub skeleton: (u32, Arc<Payload>),
    pub controls: (u32, Arc<Payload>),
    pub equipment_skeleton: (u32, Arc<Payload>),
    pub equipment_controls: (u32, Arc<Payload>),
    pub markers: (u32, Arc<Payload>),
    pub pose: (u32, Arc<Payload>),
    pub layers: (u32, Arc<Payload>),
    pub consumers: Vec<(u32, Arc<Payload>)>,
    /// Named type-5 events the carrier's own first-person clips use.
    pub vocabulary: clip::Vocabulary,
    /// A live native clip that serves as the class template for private clips.
    pub clip_template: u32,
}

pub(super) fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("Native rig identity")?,
        16,
    )?)
}

/// The weapon's rig report and the first-person attachment rows its content key selects, each as
/// the attachment owner, the arms rig entity at `+78`, the hold category at `+68` and the
/// animation profile at `+6C` that the arms rig's state selectors test. Every row of one owner
/// names the same entity, so the profile decides what each action plays.
pub(super) fn rows(native: &mut Reader, item_tag: u32) -> Result<(Value, Vec<[u32; 4]>)> {
    let report = crate::tiger::rig::inspect(native, item_tag, false)?;
    let runtime = hex(&report["runtime_entity"])?;
    let content = hex(&report["content_key"])?;
    let components = report["components"]
        .as_array()
        .context("Native weapon components")?;
    let mut rows = Vec::new();
    for component in components
        .iter()
        .filter(|c| c["class"] == "80804221" && hex(&c["entity"]).ok() == Some(runtime))
    {
        let owner = hex(&component["owner"])?;
        let attachments = native.tag(owner, Some(0x80809C36))?;
        for row in crate::tiger::rig::attachment_rows(
            &attachments,
            attachments.pointer(24)?,
            content,
            false,
        )? {
            rows.push([
                owner,
                attachments.u32(row + 0x78)?,
                attachments.u32(row + 0x68)?,
                attachments.u32(row + 0x6C)?,
            ]);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    Ok((report, rows))
}

/// The weapon's rig report and the first-person attachment owner and entity its content key
/// selects.
fn first_person(native: &mut Reader, item_tag: u32) -> Result<(Value, u32, u32)> {
    let (report, rows) = rows(native, item_tag)?;
    let mut chains = rows.iter().map(|r| (r[0], r[1])).collect::<Vec<_>>();
    chains.dedup();
    ensure!(
        chains.len() == 1,
        "Native first-person attachment is ambiguous"
    );
    let (attachment, entity) = chains[0];
    Ok((report, attachment, entity))
}

/// The single owner of a component class on an entity of a rig report.
pub(super) fn owner(report: &Value, class: &str, entity: u32) -> Result<u32> {
    let owners = report["components"]
        .as_array()
        .context("Native weapon components")?
        .iter()
        .filter(|c| c["class"] == class && hex(&c["entity"]).ok() == Some(entity))
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    let [owner] = owners.as_slice() else {
        anyhow::bail!("Native component {class} is absent or ambiguous");
    };
    Ok(*owner)
}

/// The native weapon's equipment bone names and parent indices in palette order. A model drawn on
/// the donor's own first-person animation skins to these bones.
pub(in crate::marathon) fn equipment_bones(
    native: &mut Reader,
    item_tag: u32,
) -> Result<Vec<(u32, i32)>> {
    let report = crate::tiger::rig::inspect(native, item_tag, false)?;
    let runtime = hex(&report["runtime_entity"])?;
    let owners = report["components"]
        .as_array()
        .context("Native weapon components")?
        .iter()
        .filter(|c| c["class"] == "80808546" && hex(&c["entity"]).ok() == Some(runtime))
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    let [owner] = owners.as_slice() else {
        anyhow::bail!("Native equipment skeleton is absent or ambiguous");
    };
    names(&*native.tag(*owner, Some(0x80809C36))?)
}

/// Every state name a native weapon's first-person lookup can request, without reading its
/// clips.
pub(super) fn requested_actions(native: &mut Reader, item_tag: u32) -> Result<BTreeSet<u32>> {
    let (report, _, entity) = first_person(native, item_tag)?;
    let owners = report["components"]
        .as_array()
        .context("Native weapon components")?
        .iter()
        .filter(|c| c["class"] == "8080344B" && hex(&c["entity"]).ok() == Some(entity))
        .map(|c| hex(&c["owner"]))
        .collect::<Result<Vec<_>>>()?;
    let [owner] = owners.as_slice() else {
        anyhow::bail!("Native first-person lookup is absent or ambiguous");
    };
    let lookup = native.tag(*owner, Some(0x80809C36))?;
    let states = native.tag(lookup.u32(lookup.pointer(24)? + 0x9C)?, Some(0x80803465))?;
    states
        .array(8, 8, Some(0x8080342E))?
        .into_iter()
        .map(|at| states.u32(at))
        .collect()
}

impl Carrier {
    pub fn read(native: &mut Reader, item_tag: u32) -> Result<Self> {
        let (report, attachment, entity) = first_person(native, item_tag)?;
        let runtime = hex(&report["runtime_entity"])?;
        let components = report["components"]
            .as_array()
            .context("Native weapon components")?;
        let owner_for = |class: &str, target: u32| -> Result<u32> {
            let selected = components
                .iter()
                .filter(|c| c["class"] == class && hex(&c["entity"]).ok() == Some(target))
                .collect::<Vec<_>>();
            ensure!(
                selected.len() == 1,
                "Native component {class} is absent or ambiguous"
            );
            hex(&selected[0]["owner"])
        };
        let mut owned = |class: &str, target: u32| -> Result<(u32, Arc<Payload>)> {
            let tag = owner_for(class, target)?;
            Ok((tag, native.tag(tag, Some(0x80809C36))?))
        };
        let skeleton = owned("80808546", entity)?;
        let controls = owned("80808F8F", entity)?;
        let equipment_skeleton = owned("80808546", runtime)?;
        let equipment_controls = owned("80808F8F", runtime)?;
        let markers = owned("80808507", runtime)?;
        let lookup = owned("8080344B", entity)?;
        let pose = owned("80803640", entity)?;
        let set = owned("808036CF", entity)?;
        let definition = lookup.1.pointer(24)?;
        let bank_tag = lookup.1.u32(definition + 0x90)?;
        let parameter_tag = lookup.1.u32(definition + 0x94)?;
        let state_tag = lookup.1.u32(definition + 0x9C)?;
        let bank = native.tag(bank_tag, Some(0x808036F6))?;
        let layer_tag = pose.1.u32(pose.1.pointer(24)? + 0x11C)?;
        let layers = native.tag(layer_tag, Some(0x808034EB))?;
        ensure!(
            layers.array(40, 2, Some(0x80800006))?.len()
                == bank.array(0x68, 32, Some(0x80809002))?.len(),
            "Native pose dispatch does not match its clip bank"
        );
        let consumers = vec![pose.clone(), set];
        for (tag, payload) in &consumers {
            ensure!(
                payload.u32(native_animation::bank_field(payload)?)? == bank_tag,
                "Native animation consumer {tag:08X} names another bank"
            );
        }
        let mut vocabulary = clip::Vocabulary::new();
        let mut clip_template = None;
        for at in bank.array(8, 4, Some(0x80808F48))? {
            let tag = bank.u32(at)?;
            let payload = native.tag(tag, Some(0x80808F49))?;
            vocabulary.extend(clip::native_vocabulary(&payload)?);
            clip_template.get_or_insert(tag);
        }
        Ok(Self {
            runtime,
            attachment,
            entity,
            parameters: (parameter_tag, native.tag(parameter_tag, Some(0x80808EE1))?),
            states: (state_tag, native.tag(state_tag, Some(0x80803465))?),
            lookup,
            bank: (bank_tag, bank),
            skeleton,
            controls,
            equipment_skeleton,
            equipment_controls,
            markers,
            pose,
            layers: (layer_tag, layers),
            consumers,
            vocabulary,
            clip_template: clip_template.context("Native first-person bank has no clips")?,
        })
    }

    /// Every state name the carrier's lookup can request.
    pub fn actions(&self) -> Result<BTreeSet<u32>> {
        self.states
            .1
            .array(8, 8, Some(0x8080342E))?
            .into_iter()
            .map(|at| self.states.1.u32(at))
            .collect()
    }
}

fn poses(payload: &Payload) -> Result<Vec<Pose>> {
    payload
        .array(payload.pointer(24)? + 0x90, 32, Some(0x80809F75))?
        .into_iter()
        .map(|at| {
            let mut values = [0.; 8];
            for (i, value) in values.iter_mut().enumerate() {
                *value = payload.f32(at + i * 4)?;
            }
            Pose::checked(values)
        })
        .collect()
}

pub(super) fn names(payload: &Payload) -> Result<Vec<(u32, i32)>> {
    payload
        .array(payload.pointer(24)? + 0x80, 16, Some(0x80808A08))?
        .into_iter()
        .map(|row| Ok((payload.u32(row)?, payload.u32(row + 4)? as i32)))
        .collect()
}

pub(super) struct Extension {
    pub owners: Vec<Value>,
    pub skeleton: Payload,
    pub controls: Payload,
    /// Native equipment bone names in palette order, after the extension.
    pub palette: Vec<u32>,
    pub report: Value,
}

/// Append the source weapon bones the carrier lacks to both ends of the equipment link. A
/// source bone keeps its source bind relative to the weapon root, so clip channels and geometry
/// share the source frame. Shared bones keep their native binds and receive bind corrections.
pub(super) fn extend(carrier: &Carrier, weapon: &source::Rig, graph: &Path) -> Result<Extension> {
    let equipment = &carrier.equipment_skeleton.1;
    let first_person = &carrier.skeleton.1;
    let equipment_names = names(equipment)?;
    let first_person_names = names(first_person)?;
    let mut equipment_bind = poses(equipment)?;
    let mut first_person_bind = poses(first_person)?;
    let first = first_person_names
        .len()
        .checked_sub(equipment_names.len())
        .context("Native weapon section is larger than the arm rig")?;
    for (index, (name, parent)) in equipment_names.iter().enumerate() {
        ensure!(
            first_person_names[first + index].0 == *name
                && (index == 0 || first_person_names[first + index].1 == first as i32 + parent),
            "Native equipment and first-person weapon sections differ"
        );
    }
    let mut mapped: BTreeMap<u32, usize> = equipment_names
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (*name, i))
        .collect();
    let root = *mapped
        .get(&weapon.names[0])
        .context("Native carrier lacks the source weapon root")?;
    ensure!(root == 0, "Native weapon root is not its equipment root");
    let procedural = native_animation::rig::procedural_bones(equipment)?;
    let mut equipment_extra = Vec::new();
    let mut first_person_extra = Vec::new();
    let mut appended = Vec::new();
    let mut reparented = Vec::new();
    for (bone, &name) in weapon.names.iter().enumerate() {
        if mapped.contains_key(&name) {
            continue;
        }
        let parent = weapon.parents[bone].context("Unmapped source weapon root")?;
        let mut native_parent = mapped[&weapon.names[parent]];
        // Appended FK bones cannot follow a procedural bone, which moves behind them.
        while procedural.contains(&native_parent) {
            let ancestor = usize::try_from(equipment_names[native_parent].1)
                .context("Procedural native bone has no FK ancestor")?;
            reparented.push(json!({"bone":format!("{name:08X}"),"procedural_parent":format!("{:08X}",equipment_names[native_parent].0),
                "parent":format!("{:08X}",equipment_names[ancestor].0)}));
            native_parent = ancestor;
        }
        let relative = weapon.bind[0].inverse().then(weapon.bind[bone]);
        let object = equipment_bind[root].then(relative);
        let first_person_object = first_person_bind[first + root].then(relative);
        let index = equipment_bind.len();
        mapped.insert(name, index);
        equipment_extra.push(native_animation::rig::Bone {
            name,
            parent: native_parent,
            local: equipment_bind[native_parent].inverse().then(object).0,
            object: object.0,
            inverse: object.inverse().0,
        });
        first_person_extra.push(native_animation::rig::Bone {
            name,
            parent: first + native_parent,
            local: first_person_bind[first + native_parent]
                .inverse()
                .then(first_person_object)
                .0,
            object: first_person_object.0,
            inverse: first_person_object.inverse().0,
        });
        equipment_bind.push(object);
        first_person_bind.push(first_person_object);
        appended.push(name);
    }
    ensure!(
        !equipment_extra.is_empty(),
        "Source weapon bones all exist on the carrier, so no private weapon section is required"
    );
    let placement = native_animation::rig::Placement::required(equipment)?
        .max(native_animation::rig::Placement::required(first_person)?);
    let equipment_extension = native_animation::rig::extend(
        equipment,
        carrier.equipment_skeleton.0,
        &carrier.equipment_controls.1,
        &equipment_extra,
        placement,
    )?;
    let first_person_extension = native_animation::rig::extend(
        first_person,
        carrier.skeleton.0,
        &carrier.controls.1,
        &first_person_extra,
        placement,
    )?;
    native_animation::rig::validate_weapon_pair(
        &equipment_extension.skeleton,
        &first_person_extension.skeleton,
    )?;
    let palette = names(&equipment_extension.skeleton)?
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    let folder = graph.join("animation");
    fs::create_dir_all(&folder)?;
    let mut owners = Vec::new();
    for (kind, first_person, tag, original, converted, relocation) in [
        (
            "skeleton",
            false,
            carrier.equipment_skeleton.0,
            &carrier.equipment_skeleton.1,
            &equipment_extension.skeleton,
            Some(equipment_extension.relocation),
        ),
        (
            "controls",
            false,
            carrier.equipment_controls.0,
            &carrier.equipment_controls.1,
            &equipment_extension.controls,
            None,
        ),
        (
            "skeleton",
            true,
            carrier.skeleton.0,
            &carrier.skeleton.1,
            &first_person_extension.skeleton,
            Some(first_person_extension.relocation),
        ),
        (
            "controls",
            true,
            carrier.controls.0,
            &carrier.controls.1,
            &first_person_extension.controls,
            None,
        ),
    ] {
        let template_file = format!("animation/rig-{tag:08X}-template.bin");
        let file = format!("animation/rig-{tag:08X}.bin");
        fs::write(graph.join(&template_file), &original.0)?;
        fs::write(graph.join(&file), &converted.0)?;
        owners.push(json!({"native":tag,"first_person":first_person,"kind":kind,"template_file":template_file,"file":file,
            "relocation":relocation.map(|(cut, delta)| json!({"cut":cut,"delta":delta}))}));
    }
    let report = json!({"appended":appended.iter().map(|n| format!("{n:08X}")).collect::<Vec<_>>(),
        "placement":format!("{placement:?}"),"layout":equipment_extension.layout,"reparented":reparented});
    Ok(Extension {
        owners,
        skeleton: first_person_extension.skeleton,
        controls: first_person_extension.controls,
        palette,
        report,
    })
}

/// Names the source weapon placement publishes, on any of its bones.
pub(super) fn placement_names(placement: &Payload) -> Result<BTreeSet<u32>> {
    let base = placement.pointer(24)?;
    placement
        .array(base + 0xA8, 48, None)?
        .into_iter()
        .map(|row| placement.u32(row + 0x28))
        .collect()
}

/// Rewrite the carrier's runtime markers that the source weapon also names, which include the
/// named hand constraint targets, and append the source hand targets in `appended` that the
/// carrier lacks. A source point on a nonroot bone is composed with that bone's bind into the
/// weapon root frame, so it stays at the bone's rest place. Every stock weapon marker row is
/// bound to the weapon root, and appended rows are too. Carrier-only markers keep their rows
/// and are reported.
pub(super) fn markers(
    carrier: &Carrier,
    placement: &Payload,
    weapon: &source::Rig,
    appended: &BTreeSet<u32>,
    graph: &Path,
) -> Result<Value> {
    use crate::tiger::markers;
    let carrier_rows = markers::read_native(&carrier.markers.1)?;
    let carrier_names = carrier_rows.iter().map(|m| m.name).collect::<BTreeSet<_>>();
    let base = placement.pointer(24)?;
    let mut source = Vec::new();
    let mut moved = Vec::new();
    for row in placement.array(base + 0xA8, 48, None)? {
        let name = placement.u32(row + 0x28)?;
        if !carrier_names.contains(&name) && !appended.contains(&name) {
            continue;
        }
        let bone = placement.u32(row + 0x24)? as usize;
        ensure!(
            bone < weapon.names.len(),
            "Source marker bone is outside the weapon rig"
        );
        if bone != 0 {
            moved.push(
                json!({"name":format!("{name:08X}"),"bone":format!("{:08X}", weapon.names[bone])}),
            );
        }
        let mut local = [0.; 8];
        for (i, value) in local[..4].iter_mut().enumerate() {
            *value = placement.f32(row + i * 4)?;
        }
        for (i, value) in local[4..7].iter_mut().enumerate() {
            *value = placement.f32(row + 0x10 + i * 4)?;
        }
        local[7] = 1.;
        let object = weapon.bind[0]
            .inverse()
            .then(weapon.bind[bone])
            .then(Pose::checked(local)?);
        source.push(markers::Marker {
            name,
            binding: [0, 0],
            position: [object.0[4], object.0[5], object.0[6], 1.],
            orientation: [object.0[0], object.0[1], object.0[2], object.0[3]],
            row: 0,
        });
    }
    let (converted, plan) = markers::rewrite(&carrier.markers.1, &source)?;
    let added = plan.added.iter().map(|m| m.name).collect::<BTreeSet<_>>();
    ensure!(
        added == *appended,
        "Appended hand targets differ from the source placement"
    );
    crate::tiger::markers::validate(&converted.0)?;
    let tag = carrier.markers.0;
    let file = format!("animation/markers-{tag:08X}.bin");
    let template_file = format!("animation/markers-{tag:08X}-template.bin");
    fs::write(graph.join(&file), &converted.0)?;
    fs::write(graph.join(&template_file), &carrier.markers.1.0)?;
    Ok(
        json!({"kind":"markers","native":tag,"first_person":false,"file":file,"template_file":template_file,
        "source_markers":plan.matched.iter().map(|m| format!("{:08X}", m.name)).collect::<Vec<_>>(),
        "appended_markers":added.iter().map(|n| format!("{n:08X}")).collect::<Vec<_>>(),
        "rest_place_markers":moved,
        "carrier_markers":plan.carrier_only.iter().map(|n| format!("{n:08X}")).collect::<Vec<_>>()}),
    )
}

/// A translated descriptor: the source descriptor it came from, its clip placeholder and
/// timing, and the native playback mode its composition requires.
pub(super) struct Row {
    pub source: u32,
    pub name: u32,
    pub clip: usize,
    pub duration: f32,
    pub mode: u16,
}

fn array_rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.0[p.array_range(field, stride, Some(class))?].to_vec())
}

impl Carrier {
    /// The carrier's descriptor count, which is the first translated descriptor's ordinal.
    pub fn descriptors(&self) -> Result<usize> {
        Ok(self.bank.1.array(0x68, 32, Some(0x80809002))?.len())
    }
}

/// The carrier's bank with the translated clips and descriptors after its own rows. A
/// translated descriptor's clip ordinal counts from the end of the carrier's clips.
///
/// The carrier's rows keep their ordinals. The lookup's look table and the pose layers name
/// clips by ordinal, and the untranslated states name descriptors by ordinal. A bank that held
/// only the translated rows left those ordinals past its arrays, and the client read the
/// following zero words and array markers as clip tags.
pub(super) fn bank(carrier: &Carrier, clips: &[u32], rows: &[Row]) -> Result<Payload> {
    let mut bank = (*carrier.bank.1).clone();
    let words = bank
        .0
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect::<BTreeSet<_>>();
    let mut clip_rows = array_rows(&bank, 8, 4, 0x80808F48)?;
    // One signed 32-bit word per clip, class 0x80800007. A clip without one holds -1.
    let mut auxiliary = array_rows(&bank, 0x18, 4, 0x80800007)?;
    let mut descriptors = array_rows(&bank, 0x68, 32, 0x80809002)?;
    ensure!(
        clip_rows.len() / 4 == auxiliary.len() / 4,
        "Native clip auxiliary map differs"
    );
    let first_clip = clip_rows.len() / 4;
    for clip in clips {
        ensure!(
            ![0, u32::MAX].contains(clip) && !words.contains(clip),
            "Private clip placeholder collides with a bank word"
        );
    }
    ensure!(
        clips.iter().collect::<BTreeSet<_>>().len() == clips.len(),
        "Private clip placeholders repeat"
    );
    for clip in clips {
        clip_rows.extend(clip.to_le_bytes());
        auxiliary.extend(u32::MAX.to_le_bytes());
    }
    for row in rows {
        ensure!(
            row.clip < clips.len() && row.duration.is_finite() && row.duration >= 0.,
            "Translated descriptor is invalid"
        );
        let mut bytes = [0u8; 32];
        bytes[16..20].copy_from_slice(&row.name.to_le_bytes());
        bytes[20..24].copy_from_slice(&row.duration.to_le_bytes());
        bytes[24..26].copy_from_slice(&u16::try_from(first_clip + row.clip)?.to_le_bytes());
        bytes[26..28].copy_from_slice(&row.mode.to_le_bytes());
        descriptors.extend(bytes);
    }
    append(&mut bank.0, 8, 0x80808F48, &clip_rows, 4)?;
    append(&mut bank.0, 0x18, 0x80800007, &auxiliary, 4)?;
    append(&mut bank.0, 0x68, 0x80809002, &descriptors, 32)?;
    Ok(bank)
}

/// The carrier's pose layer table with a dispatch entry for each translated descriptor. The
/// carrier's layers, routes and dispatch keep serving its own descriptors, and the animation
/// consumers keep buffers sized for those layers. Source supplemental layers are reported
/// separately.
pub(super) fn layers(carrier: &Carrier, translated: &[u16]) -> Result<Payload> {
    let mut layers = (*carrier.layers.1).clone();
    let mut dispatch = array_rows(&layers, 40, 2, 0x80800006)?;
    let routes = layers.array(56, 32, Some(0x808034EF))?.len();
    for &entry in translated {
        ensure!(
            entry == u16::MAX || usize::from(entry) < routes,
            "Translated descriptor dispatches to an absent layer route"
        );
        dispatch.extend(entry.to_le_bytes());
    }
    append(&mut layers.0, 40, 0x80800006, &dispatch, 2)?;
    Ok(layers)
}

/// The layer route the carrier's own playback of a native action selects: the dispatch entry of
/// the lowest absolute carrier descriptor its state names that has one, or `None`. Every
/// descriptor the examined carrier layers play is an additive clip, and those additive
/// descriptors dispatch to no route themselves, so a route adds sway and locomotion on top of
/// the base pose rather than replacing it.
pub(super) fn carrier_layer_route(
    carrier: &Carrier,
    native: &mut Reader,
    action: u32,
) -> Result<Option<u16>> {
    if !carrier.actions()?.contains(&action) {
        return Ok(None);
    }
    let bank = &carrier.bank.1;
    let rows = bank.array(0x68, 32, Some(0x80809002))?;
    let clips = bank.array(8, 4, Some(0x80808F48))?;
    let dispatch = carrier.layers.1.array(40, 2, Some(0x80800006))?;
    for descriptor in native_animation::state::descriptors(&carrier.states.1, action)? {
        let entry = carrier.layers.1.u16(
            *dispatch
                .get(descriptor)
                .context("Carrier dispatch does not cover its descriptors")?,
        )?;
        let ordinal = bank.u16(*rows.get(descriptor).context("Carrier descriptor")? + 24)?;
        if entry == u16::MAX || ordinal == u16::MAX {
            continue;
        }
        let clip = native.tag(
            bank.u32(
                *clips
                    .get(usize::from(ordinal))
                    .context("Carrier clip ordinal")?,
            )?,
            Some(0x80808F49),
        )?;
        if clip.u8(0x181)? == 0 {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// A named route of weighted descriptor ordinals in the private bank.
pub(super) type Routes = BTreeMap<u32, Vec<(u32, f32)>>;

/// The carrier's state table `80803465` with each translated action routed through a private
/// weighted node. Names that select the same weighted descriptors share a node. Every other
/// name keeps its native node, which addresses the carrier's descriptors at their original
/// ordinals and reads the carrier's dictionary.
pub(super) fn states(carrier: &Carrier, routes: &Routes) -> Result<Payload> {
    let p = &carrier.states.1;
    let mut out = p.0.clone();
    let mut nodes = p
        .array(24, 16, Some(0x8080342F))?
        .into_iter()
        .map(|row| {
            Ok((
                p.u64(row)?,
                if p.u64(row + 8)? == 0 {
                    None
                } else {
                    Some(p.pointer(row + 8)?)
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut shared: Vec<(&Vec<(u32, f32)>, usize)> = Vec::new();
    let mut targets = BTreeMap::new();
    for (name, route) in routes {
        ensure!(!route.is_empty(), "Translated action route is empty");
        let index = if let Some(&(_, index)) = shared.iter().find(|(r, _)| *r == route) {
            index
        } else {
            let at = (out.len() + 19) & !15;
            out.resize(at - 4, 0);
            out.extend(0x80803434u32.to_le_bytes());
            out.extend([0; 16]);
            let entries = route
                .iter()
                .flat_map(|(descriptor, weight)| {
                    descriptor
                        .to_le_bytes()
                        .into_iter()
                        .chain(weight.to_le_bytes())
                })
                .collect::<Vec<_>>();
            append(&mut out, at, 0x80803439, &entries, 8)?;
            nodes.push((1, Some(at)));
            shared.push((route, nodes.len() - 1));
            nodes.len() - 1
        };
        targets.insert(*name, index);
    }
    append(&mut out, 24, 0x8080342F, &vec![0; nodes.len() * 16], 16)?;
    for (field, (kind, target)) in Payload(out.clone())
        .array(24, 16, Some(0x8080342F))?
        .into_iter()
        .zip(nodes)
    {
        put(&mut out, field, &kind.to_le_bytes())?;
        put(
            &mut out,
            field + 8,
            &target
                .map_or(0, |target| target as i64 - field as i64 - 8)
                .to_le_bytes(),
        )?;
    }
    let mut routed = BTreeSet::new();
    for field in p.array(8, 8, Some(0x8080342E))? {
        let name = p.u32(field)?;
        if let Some(&index) = targets.get(&name) {
            put(&mut out, field + 4, &u32::try_from(index)?.to_le_bytes())?;
            routed.insert(name);
        }
    }
    ensure!(
        routed.len() == targets.len(),
        "Translated action is absent from the native state table"
    );
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(Payload(out))
}
