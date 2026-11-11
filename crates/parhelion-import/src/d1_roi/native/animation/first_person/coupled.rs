//! Rise of Iron action-coupled pose layers on the native table, so each translated descriptor plays
//! the coupled layers its own source route plays, with the source's clips and drivers.
//!
//! Rise of Iron keeps its own supplemental layers (`80802919`) beside its bank. A weapon whose
//! actions mix the family's shared clips with clips of its own bridges their different rest poses
//! with the settle layer `FC23AA50`, a constant additive offset routed only on the shared clips.
//! The donor's layer of that name was authored against the donor's clips, so a translated action
//! takes the source layer's choices in place of the donor's, appended to the native layer of the
//! same name. The source option is the native option without its condition array, and its drivers
//! name parameters that both libraries declare. A donor layer whose offset the source settle
//! folds in ([`FOLDED`]) leaves the same descriptors.
//!
//! A coupled layer the source table lacks is one no source route plays, and the source clips hold
//! the motion it adds. Shadowkeep splits a sprint loop into a nearly still clip and the additive
//! sway layer `4D467CC1`, while each Rise of Iron sprint loop is that clip with the sway already
//! applied. Such a donor layer therefore leaves every descriptor that plays a source clip
//! ([`lacked`]), as one the source route leaves out does.
use super::*;
use crate::{
    presentation::append,
    tiger::animation::{
        clip, controller,
        layer::{ACTION_COUPLED, LayerOption, append_choices},
    },
};

/// Rise of Iron pose resource. Its bank is at `+108`, its parameter library at `+10C` and its
/// supplemental layer table at `+118`.
const SOURCE_POSE: u32 = 0x8080_2790;
const SOURCE_LIBRARY: u32 = 0x8080_29E1;
const SOURCE_LAYERS: u32 = 0x8080_2919;

/// Native layers whose offset a source coupled layer folds in, as source and native layer names.
/// Shadowkeep's alert layer `2B059A81` adds the relaxed stance offset on the donor's relaxed
/// actions. Rise of Iron has no such layer. On its relaxed actions the settle `FC23AA50` plays one
/// constant offset that holds both the weapon's rest offset and the relaxed offset, so a
/// descriptor taking the source settle would apply the relaxed offset twice with the donor's alert
/// layer.
const FOLDED: [(u32, u32); 1] = [(0xFC23_AA50, 0x2B05_9A81)];

pub(super) struct Inputs<'a> {
    pub source: &'a Source,
    pub runtime: &'a Value,
    pub mapping: &'a controls::Mapping,
    pub library: &'a event::Library,
    /// The donor's own bank, whose clips witness source hand constraints.
    pub bank: &'a Payload,
    /// The source descriptors each native descriptor stands for, from named actions replaced in
    /// place and from private routes.
    pub sources: &'a BTreeMap<usize, BTreeSet<usize>>,
}

/// The couplings each translated descriptor takes, for
/// [`crate::tiger::animation::layer::Translated::own`], the choices they name and the bank holding
/// their clips.
pub(super) struct Own {
    pub descriptors: BTreeMap<usize, BTreeMap<u32, Vec<u16>>>,
    /// Choices for native layers, by name, with the index the first takes. They follow the layer's
    /// current choices, so routes can name them before [`Self::append`] adds them.
    choices: Vec<(u32, u16, Vec<Vec<LayerOption>>)>,
    pub bank: Payload,
    /// Converted layer clips, as extra clip records.
    pub clips: Vec<Value>,
    pub report: Value,
}

impl Own {
    /// `table` with the planned choices appended.
    pub fn append(&self, table: &Payload) -> Result<Payload> {
        let mut table = table.clone();
        for (name, first, choices) in &self.choices {
            let (next, _, at) = append_choices(&table, *name, choices)?;
            ensure!(
                at == *first,
                "Pose layer {name:08X} gained choices before the source's were appended"
            );
            table = next;
        }
        Ok(table)
    }
}

/// The source's pose tables: its layers, the bank they index and its parameter names.
struct Tables {
    layers: Payload,
    bank: Payload,
    /// Global and local parameters, as name and the byte after the kind, of the source and then
    /// of the native first-person controller.
    parameters: [Vec<(u32, u8)>; 2],
    native: [Vec<(u32, u8)>; 2],
    tags: [u32; 3],
}

fn tables(inputs: &Inputs) -> Result<Option<Tables>> {
    let source = inputs.source;
    let Some(owner) = controls::optional_owner(inputs.runtime, true, SOURCE_POSE, true)? else {
        return Ok(None);
    };
    let pose = source.read(owner)?;
    let at = pose.pointer(24)?;
    ensure!(
        pose.u32(at - 4)? == SOURCE_POSE,
        "Source pose resource differs"
    );
    let lookup = source.read(controls::owner(inputs.runtime, true, SOURCE_LOOKUP, true)?)?;
    let bank = pose.u32(at + 0x108)?;
    ensure!(
        lookup.u32(lookup.pointer(24)? + 0xA8)? == bank,
        "Source pose and lookup name different banks"
    );
    let read = |offset: usize, class: u32| -> Result<(u32, Payload)> {
        let tag = pose.u32(at + offset)?;
        ensure!(
            source.reference(tag)? == class,
            "Source pose table {tag:08X} class differs"
        );
        Ok((tag, source.read(tag)?))
    };
    let (library_tag, library) = read(0x10C, SOURCE_LIBRARY)?;
    let (layers_tag, layers) = read(0x118, SOURCE_LAYERS)?;
    let mut parameters = [Vec::new(), Vec::new()];
    for (names, field) in parameters.iter_mut().zip([8, 0x18]) {
        for row in library.array(field, 16, Some(0x8080_20B4))? {
            names.push((library.u32(row)?, library.u8(row + 5)?));
        }
    }
    let bank = source.read(bank)?;
    ensure!(
        layers.u64(40)? == bank.u64(0x68)?,
        "Source pose dispatch does not cover its bank"
    );
    Ok(Some(Tables {
        layers,
        bank,
        parameters,
        native: [Vec::new(), Vec::new()],
        tags: [owner, library_tag, layers_tag],
    }))
}

/// For each coupled source layer, by name, the options the source route of `descriptor` plays.
/// Source route edges (`808028E6`) hold the option, then the layer.
fn couplings(
    t: &Tables,
    descriptor: usize,
    layers: &BTreeMap<usize, (u32, usize)>,
) -> Result<BTreeMap<u32, Vec<u16>>> {
    let mut result = layers
        .values()
        .map(|&(name, _)| (name, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let alias = t.layers.u16(
        *t.layers
            .array(40, 2, Some(0x8080_0006))?
            .get(descriptor)
            .context("Source pose descriptor")?,
    )?;
    if alias == u16::MAX {
        return Ok(result);
    }
    let route = *t
        .layers
        .array(56, 32, Some(0x8080_254E))?
        .get(usize::from(alias))
        .context("Source pose route")?;
    for edge in t.layers.array(route, 4, Some(0x8080_28E6))? {
        if let Some((name, _)) = layers.get(&usize::from(t.layers.u16(edge + 2)?)) {
            result
                .get_mut(name)
                .context("Source coupled layer")?
                .push(t.layers.u16(edge)?);
        }
    }
    Ok(result)
}

/// A packed source parameter reference as the native reference to the parameter of the same
/// name. Bit 16 selects the global array in both versions, and a low word of `FFFF` names none.
/// Kind numbers differ between the versions, so only the name and the byte after the kind match.
fn parameter(reference: u32, from: &[Vec<(u32, u8)>; 2], to: &[Vec<(u32, u8)>; 2]) -> Result<u32> {
    if reference & 0xFFFF == 0xFFFF {
        return Ok(reference);
    }
    ensure!(
        reference >> 17 == 0 && reference & 0xFFFF <= i16::MAX as u32,
        "Source pose parameter reference {reference:08X} has unknown bits"
    );
    let array = usize::from(reference & 0x1_0000 == 0);
    let key = *from[array]
        .get((reference & 0xFFFF) as usize)
        .context("Source pose parameter is outside its library")?;
    let found = to[array]
        .iter()
        .enumerate()
        .filter(|(_, row)| **row == key)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Source pose parameter {:08X} has no unique native counterpart",
        key.0
    );
    Ok(reference & 0x1_0000 | u32::try_from(found[0])?)
}

/// Bank slots for the source clips coupled layers play.
#[derive(Clone)]
struct Clips<'a, 'b> {
    inputs: &'a Inputs<'b>,
    /// The tag each bank slot holds, and the name its clip carries.
    slots: Vec<u32>,
    names: Vec<u32>,
    /// Slot and storage size of each source tag the bank already holds or that was converted.
    done: BTreeMap<u32, (u32, u32)>,
    added: Vec<(Value, Payload)>,
}

impl<'a, 'b> Clips<'a, 'b> {
    fn new(
        inputs: &'a Inputs<'b>,
        native: &mut Reader,
        graph: &Path,
        bank: &Payload,
        appended: &[Value],
    ) -> Result<Self> {
        let extra = appended
            .iter()
            .map(|clip| {
                Ok((
                    u32::try_from(clip["source"].as_u64().context("Appended clip tag")?)?,
                    clip["file"].as_str().context("Appended clip file")?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut clips = Self {
            inputs,
            slots: Vec::new(),
            names: Vec::new(),
            done: BTreeMap::new(),
            added: Vec::new(),
        };
        for row in bank.array(8, 4, Some(0x8080_8F48))? {
            let tag = bank.u32(row)?;
            let payload = if let Some(file) = extra.get(&tag) {
                let payload = Payload(fs::read(graph.join(file))?);
                let slot = u32::try_from(clips.slots.len())?;
                clips.done.insert(tag, (slot, clip::size(&payload)?));
                payload
            } else {
                (*native.tag(tag, Some(0x8080_8F49))?).clone()
            };
            clips.slots.push(tag);
            clips.names.push(payload.u32(0x120)?);
        }
        Ok(clips)
    }

    /// Source layer clip `original` on the native layout. A clip that binds hand constraints keeps
    /// them through a witness, as action clips do: the first donor bank clip of its blend mode
    /// whose bindings admit the source rows, else the layer's native clip `template`, which drops
    /// them when it binds none.
    fn convert(
        &self,
        native: &mut Reader,
        original: &Payload,
        template: u32,
    ) -> Result<(Payload, Value)> {
        let library = self.inputs.library;
        if original.u64(0x30)? == 0 {
            return convert_events(original, None, Some(library));
        }
        let bones = &self.inputs.mapping.bone_map;
        if let Some(converted) = witnessed(native, self.inputs.bank, original, bones, library)? {
            return Ok(converted);
        }
        let template = native.tag(template, Some(0x8080_8F49))?;
        convert_events(original, Some((&template, bones)), Some(library))
    }

    /// The bank slot and storage size of source clip `tag`, converted on first use. A name another
    /// bank clip carries takes a private one.
    fn slot(
        &mut self,
        native: &mut Reader,
        tag: u32,
        template: u32,
        layer: u32,
    ) -> Result<(u32, u32)> {
        if let Some(&slot) = self.done.get(&tag) {
            return Ok(slot);
        }
        ensure!(
            !self.slots.contains(&tag),
            "Source layer clip symbol collides with a native tag"
        );
        let source = self.inputs.source;
        ensure!(
            source.reference(tag)? == SOURCE_CLIP,
            "Source layer clip is not a clip"
        );
        let original = source.read(tag)?;
        self.inputs.library.validate_source(source, &original)?;
        let (payload, mut report) = self.convert(native, &original, template)?;
        let (mut payload, slots) = self.inputs.mapping.apply(&original, &payload)?;
        report["control_mapping"] = slots;
        let source_name = payload.u32(0x120)?;
        let names = &self.names;
        let name = if names.contains(&source_name) {
            conditional::private_name(tag, |name| names.contains(&name))?
        } else {
            source_name
        };
        put(&mut payload.0, 0x120, &name.to_le_bytes());
        let slot = u32::try_from(self.slots.len())?;
        let size = clip::size(&payload)?;
        let file = format!("animation/first-person-layer-{tag:08X}.bin");
        self.slots.push(tag);
        self.names.push(name);
        self.done.insert(tag, (slot, size));
        self.added.push((
            json!({"source":tag,"template":template,"file":file,
            "frames":payload.u16(0x13C)?,"source_name":source_name,"emitted_name":name,
            "layer":layer,"report":report}),
            payload,
        ));
        Ok((slot, size))
    }

    /// The bank with the converted clips' slots, which take no auxiliary word, and their
    /// records, after writing their payloads.
    fn finish(self, bank: &Payload, graph: &Path) -> Result<(Payload, Vec<Value>)> {
        if self.added.is_empty() {
            return Ok((bank.clone(), Vec::new()));
        }
        let mut bank = bank.clone();
        let tags = self
            .slots
            .iter()
            .flat_map(|tag| tag.to_le_bytes())
            .collect::<Vec<_>>();
        let mut auxiliary = conditional::rows(&bank, 24, 4, 0x8080_0007)?;
        auxiliary.resize(self.slots.len() * 4, 0xFF);
        append(&mut bank.0, 8, 0x8080_8F48, &tags, 4)?;
        append(&mut bank.0, 24, 0x8080_0007, &auxiliary, 4)?;
        let mut records = Vec::new();
        for (record, payload) in self.added {
            fs::write(
                graph.join(record["file"].as_str().context("Layer clip file")?),
                &payload.0,
            )?;
            records.push(record);
        }
        Ok((bank, records))
    }
}

/// The native option for source option `at`. The source option (`80802744`, 48 bytes) is the
/// native option (`80803730`, 64 bytes) without the condition array at native `+10`. From source
/// `+10`: driver kind (1 for the constant weight at `+14`, 2 for the parameter at `+18`), a zero
/// word, rate, duration, the phase parameter at `+28`, flags and the playback mode at `+2F`.
/// Weighted clips at `+00` (`80802687`) are 12-byte rows of ordinal, cumulative weight and
/// storage size, as native rows (`80803737`) are.
fn option(
    t: &Tables,
    at: usize,
    mut slot: impl FnMut(u32) -> Result<(u32, u32)>,
) -> Result<LayerOption> {
    let mut playback = t.layers.bytes::<32>(at + 16)?;
    ensure!(
        matches!(t.layers.u32(at + 16)?, 1 | 2) && t.layers.u32(at + 28)? == 0 && playback[31] <= 3,
        "Source layer option driver or playback differs"
    );
    for field in [8, 24] {
        let reference = u32::from_le_bytes(playback[field..field + 4].try_into()?);
        let mapped = parameter(reference, &t.parameters, &t.native)?;
        playback[field..field + 4].copy_from_slice(&mapped.to_le_bytes());
    }
    let clips = t.bank.array(8, 4, Some(SOURCE_BANK_ROW))?;
    let mut weighted = Vec::new();
    // The converted clip's own codec spans supply the size word.
    for row in t.layers.array(at, 12, Some(0x8080_2687))? {
        let tag = t.bank.u32(
            *clips
                .get(t.layers.u32(row)? as usize)
                .context("Source layer clip ordinal")?,
        )?;
        let (index, size) = slot(tag)?;
        weighted.push((index, t.layers.f32(row + 4)?, size));
    }
    ensure!(!weighted.is_empty(), "Source layer option plays no clip");
    Ok(LayerOption {
        clips: weighted,
        playback,
    })
}

/// The first weighted clip of native layer `name`, the storage template of its new clips.
fn template(table: &Payload, bank: &Payload, name: u32) -> Result<u32> {
    let clips = bank.array(8, 4, Some(0x8080_8F48))?;
    for layer in table.array(8, 24, Some(0x8080_3724))? {
        if table.u32(layer + 16)? != name {
            continue;
        }
        for choice in table.array(layer, 16, Some(0x8080_372E))? {
            for option in table.array(choice, 64, Some(0x8080_3730))? {
                if let Some(&weight) = table.array(option, 12, Some(0x8080_3737))?.first() {
                    return bank.u32(
                        *clips
                            .get(table.u32(weight)? as usize)
                            .context("Native layer clip ordinal")?,
                    );
                }
            }
        }
    }
    anyhow::bail!("Native layer {name:08X} plays no clip")
}

/// The parameter names of the native first-person pose controller's library (definition `+110`).
fn native_parameters(native: &mut Reader, rig: &Value) -> Result<[Vec<(u32, u8)>; 2]> {
    let owner = controls::owner(rig, true, 0x8080_3640, false)?;
    let controller = native.tag(owner, Some(0x8080_9C36))?;
    let library = controller.u32(controller.pointer(24)? + 0x110)?;
    let library = native.tag(library, Some(0x8080_364C))?;
    controller::parameter_names(&library)
}

/// The coupled source route of every translated descriptor whose source descriptors all route the
/// same options, and the descriptors whose source descriptors disagree.
fn chosen(
    t: &Tables,
    inputs: &Inputs,
    translated: &BTreeSet<usize>,
    layers: &BTreeMap<usize, (u32, usize)>,
) -> Result<(ControllerChoices, Vec<usize>)> {
    let mut chosen = BTreeMap::new();
    let mut ambiguous = Vec::new();
    for &descriptor in translated {
        let Some(sources) = inputs.sources.get(&descriptor) else {
            continue;
        };
        let mut routes = sources
            .iter()
            .map(|&source| couplings(t, source, layers))
            .collect::<Result<BTreeSet<_>>>()?;
        match (routes.len(), routes.pop_first()) {
            (1, Some(route)) => {
                chosen.insert(descriptor, route);
            }
            _ => ambiguous.push(descriptor),
        }
    }
    Ok((chosen, ambiguous))
}

/// Gives every descriptor that takes a source layer of [`FOLDED`] an empty entry for the native
/// layer it folds in, so its route drops the donor's layer, where the native table has that layer
/// and the source table does not. Returns the native names folded.
fn fold(
    descriptors: &mut BTreeMap<usize, BTreeMap<u32, Vec<u16>>>,
    native: &BTreeSet<u32>,
    source: &BTreeSet<u32>,
) -> Vec<String> {
    let folded = FOLDED
        .into_iter()
        .filter(|(_, name)| native.contains(name) && !source.contains(name))
        .collect::<Vec<_>>();
    for own in descriptors.values_mut() {
        for &(from, to) in &folded {
            if own.contains_key(&from) {
                own.entry(to).or_default();
            }
        }
    }
    folded
        .iter()
        .map(|(_, name)| format!("{name:08X}"))
        .collect()
}

/// Gives every descriptor of `translated` an empty entry for each [`ACTION_COUPLED`] layer the
/// native table has and the source table lacks, so its route drops the donor's layer whether or
/// not that layer fits its clip. The source authored its clips without such a layer, so they
/// already hold its motion. In three inspected families the source sprint loop matches the donor's
/// loop with the sway layer `4D467CC1` applied to within 0.016 m on every arm control the sway
/// moves, and the donor's loop moves those controls 0.016 m or less. Keeping the donor's layer over
/// the source loop plays the sway twice, up to 0.16 m and 26 degrees further. Returns the native
/// names dropped.
fn lacked(
    descriptors: &mut BTreeMap<usize, BTreeMap<u32, Vec<u16>>>,
    translated: &BTreeSet<usize>,
    native: &BTreeSet<u32>,
    source: &BTreeSet<u32>,
) -> Vec<String> {
    let lacked = ACTION_COUPLED
        .into_iter()
        .filter(|name| native.contains(name) && !source.contains(name))
        .collect::<Vec<_>>();
    if !lacked.is_empty() {
        for &descriptor in translated {
            let own = descriptors.entry(descriptor).or_default();
            for &name in &lacked {
                own.entry(name).or_default();
            }
        }
    }
    lacked.iter().map(|name| format!("{name:08X}")).collect()
}

/// The number of choices of native first-family layer `name`.
fn choice_count(table: &Payload, name: u32) -> Result<usize> {
    for layer in table.array(8, 24, Some(0x8080_3724))? {
        if table.u32(layer + 16)? == name {
            return Ok(table.array(layer, 16, Some(0x8080_372E))?.len());
        }
    }
    anyhow::bail!("Native pose layer {name:08X} is absent")
}

/// The native choices for source options `used` of the source layer at `at`, to follow the
/// choices native layer `name` of `table` has, with each source option's native choice.
fn place(
    t: &Tables,
    (clips, native): (&mut Clips, &mut Reader),
    table: &Payload,
    (name, at): (u32, usize),
    used: &BTreeSet<u16>,
    template: u32,
) -> Result<LayerChoices> {
    let options = t.layers.array(at, 48, Some(0x8080_2744))?;
    let mut choices = Vec::new();
    for &used in used {
        let at = *options
            .get(usize::from(used))
            .context("Source layer option")?;
        choices.push(vec![option(t, at, |tag| {
            clips.slot(native, tag, template, name)
        })?]);
    }
    let first = u16::try_from(choice_count(table, name)?)?;
    ensure!(
        usize::from(first) + choices.len() <= i16::MAX as usize,
        "Pose layer choices exceed native limits"
    );
    Ok((choices, used.iter().copied().zip(first..).collect()))
}

/// Translates the coupled source layers the routes of `translated` descriptors play. Each source
/// first-family layer named in [`ACTION_COUPLED`] whose name the native table also has is
/// translated whole or not at all, and only the options those routes play become choices. A
/// descriptor takes its source couplings only when every source descriptor it stands for routes
/// the same options. Coupled layers the source table lacks leave every translated descriptor
/// ([`lacked`]). Other descriptors and layers that fail to translate keep the fit rule of
/// [`crate::tiger::animation::layer::omit_action_coupled`].
#[allow(clippy::too_many_arguments)]
pub(super) fn translate(
    inputs: &Inputs,
    native: &mut Reader,
    rig: &Value,
    graph: &Path,
    table: &Payload,
    bank: &Payload,
    translated: &BTreeSet<usize>,
    appended: &[Value],
) -> Result<Own> {
    let Some(mut t) = tables(inputs)? else {
        return Ok(Own {
            descriptors: BTreeMap::new(),
            choices: Vec::new(),
            bank: bank.clone(),
            clips: Vec::new(),
            report: json!({"status":"no source pose layers"}),
        });
    };
    t.native = native_parameters(native, rig)?;
    let native_names = table
        .array(8, 24, Some(0x8080_3724))?
        .into_iter()
        .map(|at| table.u32(at + 16))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut layers = BTreeMap::new();
    let mut source_names = BTreeSet::new();
    for (index, at) in t
        .layers
        .array(8, 24, Some(0x8080_22CE))?
        .into_iter()
        .enumerate()
    {
        let name = t.layers.u32(at + 16)?;
        source_names.insert(name);
        if ACTION_COUPLED.contains(&name) && native_names.contains(&name) {
            layers.insert(index, (name, at));
        }
    }
    let (chosen, ambiguous) = chosen(&t, inputs, translated, &layers)?;
    let mut clips = Clips::new(inputs, native, graph, bank, appended)?;
    let mut planned = Vec::new();
    let mut placed = BTreeMap::new();
    let mut report = Vec::new();
    for &layer in layers.values() {
        let name = layer.0;
        let used = chosen
            .values()
            .filter_map(|own| own.get(&name))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        if used.is_empty() {
            placed.insert(name, BTreeMap::new());
            continue;
        }
        let saved = clips.clone();
        let attempt = template(table, bank, name).and_then(|template| {
            place(
                &t,
                (&mut clips, &mut *native),
                table,
                layer,
                &used,
                template,
            )
        });
        match attempt {
            Ok((choices, map)) => {
                report.push(json!({"layer":format!("{name:08X}"),"source_options":used,
                    "native_choices":map.values().collect::<Vec<_>>()}));
                let first = *map.values().next().context("Planned pose layer choice")?;
                planned.push((name, first, choices));
                placed.insert(name, map);
            }
            Err(error) => {
                clips = saved;
                report.push(json!({"layer":format!("{name:08X}"),"reason":format!("{error:#}")}));
            }
        }
    }
    // A layer that failed keeps the donor's layer by the fit rule, so it is left out here.
    let mut descriptors = chosen
        .into_iter()
        .map(|(descriptor, own)| {
            let own = own
                .into_iter()
                .filter_map(|(name, options)| {
                    let map = placed.get(&name)?;
                    let choices = options.iter().map(|option| map.get(option).copied());
                    Some((name, choices.collect::<Option<Vec<_>>>()?))
                })
                .collect::<BTreeMap<_, _>>();
            (descriptor, own)
        })
        .filter(|(_, own)| !own.is_empty())
        .collect::<BTreeMap<_, _>>();
    let folded = fold(&mut descriptors, &native_names, &source_names);
    let coupled = descriptors.len();
    let lacked = lacked(&mut descriptors, translated, &native_names, &source_names);
    let (bank, clips) = clips.finish(bank, graph)?;
    Ok(Own {
        report: json!({"pose":t.tags[0],"library":t.tags[1],"layers":t.tags[2],"translated":report,
            "folded":folded,"lacked":lacked,"descriptors":coupled,"ambiguous_descriptors":ambiguous}),
        descriptors,
        choices: planned,
        bank,
        clips,
    })
}

type ControllerChoices = BTreeMap<usize, BTreeMap<u32, Vec<u16>>>;
type LayerChoices = (Vec<Vec<LayerOption>>, BTreeMap<u16, u16>);
