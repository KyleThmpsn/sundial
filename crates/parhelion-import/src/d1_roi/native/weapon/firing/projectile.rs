//! Source-selected retained projectiles with checked response operands and native compatibility.
use super::*;
use sha2::{Digest, Sha256};
use sundial::package_authoring::{ability_damage, entity::weapon_component_bindings, runtime};

mod align;
mod bank;
mod edit;
mod graph;
mod launch;
mod material;
mod release;
mod response;
mod selection;
mod tree;

const CONTENT: u32 = 0x5F0DD954;
const MOVEMENT: u32 = 0x0437756D;
/// Category dictionaries named by profile category rows at `+10`.
const SOURCE_CATEGORIES: u32 = 0x80AAC26E;
const NATIVE_CATEGORIES: u32 = 0x80C70CA1;

/// What a carrier profile's private copy takes from its source: the damage type, the category
/// names and the bodies of its push and `80802B30` responses, each where it differs from the
/// carrier's. A body is kept by its offset in the carrier profile.
#[derive(Clone, Default, PartialEq)]
pub(super) struct Private {
    pub damage_type: Option<u8>,
    pub categories: Option<Vec<u32>>,
    pub bodies: Vec<(usize, Vec<u8>)>,
    /// The source's amount bounds and random extent, three Float32 values.
    pub amount: Option<[u8; 12]>,
}

impl Private {
    /// Whether the copy exists only for response bodies, which the carrier's own can stand in for.
    pub fn bodies_only(&self) -> bool {
        self.damage_type.is_none()
            && self.categories.is_none()
            && self.amount.is_none()
            && !self.bodies.is_empty()
    }
}

/// The source body a carrier response takes, when the two differ. Push `808023B0` with
/// `80804B57` (40 bytes) and `80802B30` with `80804B56` (12 bytes) share one layout. The
/// push body's resource word and the other's leading word must agree, so only scalars change.
fn response_body(
    class: u32,
    s: &Payload,
    sa: usize,
    n: &Payload,
    na: usize,
) -> Result<Option<Vec<u8>>> {
    let (size, fixed) = match class {
        0x808023B0 => (40, 32..36),
        0x80802B30 => (12, 0..4),
        _ => return Ok(None),
    };
    let from =
        s.0.get(sa..sa + size)
            .context("Source profile response body")?;
    let to =
        n.0.get(na..na + size)
            .context("Native profile response body")?;
    Ok((from != to && from[fixed.clone()] == to[fixed]).then(|| from.to_vec()))
}

fn fingerprint(tag: u32, p: &Payload) -> Value {
    json!({"tag":tag,"sha256":hex::encode(Sha256::digest(&p.0))})
}

/// Qualify a source profile against the carrier's. Amount bounds, extent and the response family
/// must agree. The damage type may differ, because both versions use the encoding Kinetic 0,
/// Solar 1, Arc 2, Void 3. The returned type then replaces the carrier's in a private copy, as a
/// Rise of Iron sidearm's Arc direct hit does over carrier `8152AE89`. Category names may differ
/// when the native dictionary lists every source name, and the private copy then declares the
/// source's.
fn profile(
    source: &Source,
    native: &mut Reader,
    (from, to): (u32, u32),
    graphs: &response::Counterparts,
) -> Result<(Value, Value, Private)> {
    ensure!(
        source.reference(from)? == 0x80800861,
        "Source response is not a profile owner"
    );
    let s = source.read(from)?;
    let n = native.tag(to, Some(0x80809C36))?;
    let (sd, nd) = (s.pointer(24)?, n.pointer(24)?);
    let declaration = source.read(s.u32(sd - 4)?)?;
    ensure!(
        declaration.u32(16)? == 0x80802364
            && declaration.u32(20)? == 0x120
            && n.u32(nd - 4)? == 0x80804B02,
        "Projectile profile layout differs"
    );
    let amount_offset = s.pointer(sd + 24)?;
    let (source_amount, native_amount) = (s.bytes::<13>(amount_offset)?, n.bytes::<13>(nd + 20)?);
    // A private copy needs the checked native profile shape. With it the copy can take the
    // source's amount record, which both versions store as the same three floats.
    let copyable = ability_damage::profile(to, &n.0).is_some_and(|shape| shape.root == nd);
    let bounds = (source_amount[..12] != native_amount[..12])
        .then(|| <[u8; 12]>::try_from(&source_amount[..12]))
        .transpose()?;
    ensure!(
        (bounds.is_none() || copyable) && n.u8(nd + 16)? == 0 && n.u8(nd + 0x53)? == 0,
        "Projectile amount or native scaling differs"
    );
    for at in [amount_offset, amount_offset + 4, amount_offset + 8] {
        s.f32(at)?;
    }
    let retype = (source_amount[12] != native_amount[12]).then_some(source_amount[12]);
    if let Some(mode) = retype {
        ensure!(
            mode <= ability_damage::VOID
                && ability_damage::profile(to, &n.0)
                    .is_some_and(|shape| shape.root == nd && shape.mode == native_amount[12]),
            "Projectile damage type or native profile shape is unqualified"
        );
    }
    // A category row names a dictionary entry at `+00` and its dictionary at `+10`, the other
    // bytes clear. Both dictionaries list `BCC56192` and `B79BB03C`, which Vestian Dynasty
    // profile `80AADD83` declares where its carrier profile `8152AE89` declares only the first.
    let categories = |p: &Payload, at, dictionary| -> Result<Vec<u32>> {
        p.array(at, 24, None)?
            .into_iter()
            .map(|r| {
                ensure!(
                    p.bytes::<12>(r + 4)? == [0; 12]
                        && p.u32(r + 16)? == dictionary
                        && p.u32(r + 20)? == 0,
                    "Projectile profile category row differs"
                );
                p.u32(r)
            })
            .collect()
    };
    let from_categories = categories(&s, sd + 0xC8, SOURCE_CATEGORIES)?;
    let to_categories = categories(&n, nd + 0x60, NATIVE_CATEGORIES)?;
    let recategorized = from_categories.iter().collect::<BTreeSet<_>>()
        != to_categories.iter().collect::<BTreeSet<_>>();
    if recategorized {
        let dictionary = native.tag(NATIVE_CATEGORIES, None)?;
        let names = dictionary
            .array(8, 4, Some(0x80800070))?
            .into_iter()
            .map(|at| dictionary.u32(at))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            from_categories.iter().all(|name| names.contains(name))
                && ability_damage::profile(to, &n.0).is_some_and(|shape| shape.root == nd),
            "Projectile profile categories differ"
        );
    }
    let sr = s.array(sd + 0x28, 8, None)?;
    let nr = n.array(nd + 0x78, 8, None)?;
    // Responses pair by position and class. A graph response, Rise of Iron `8080218E` and
    // Shadowkeep `80804B54`, has the selector 8 layout: a graph at `+08` and a label filter at
    // `+10`. It must name the native graph its source graph pairs with in the tree, as sniper
    // profile `80A6A519` names `80A6A888` where carrier profile `81533A85` names `80BDE3A8`.
    let counterpart = |class: u32| match class {
        0x808027D4 => Some(0x80804B62),
        0x808023B0 => Some(0x80804B57),
        0x808021B3 => Some(0x80804B46),
        0x80802B30 => Some(0x80804B56),
        0x8080218E => Some(0x80804B54),
        _ => None,
    };
    ensure!(
        !sr.is_empty() && nr.len() >= sr.len(),
        "Projectile profile response family differs"
    );
    let mut bodies = Vec::new();
    for (srow, nrow) in sr.iter().zip(&nr) {
        let (sa, na) = (s.pointer(*srow)?, n.pointer(*nrow)?);
        let class = s.u32(sa - 4)?;
        ensure!(
            counterpart(class) == Some(n.u32(na - 4)?),
            "Projectile profile response order differs"
        );
        if copyable && let Some(body) = response_body(class, &s, sa, &n, na)? {
            bodies.push((na, body));
        }
        if class == 0x8080218E {
            let (from, to) = (s.u32(sa + 8)?, n.u32(na + 8)?);
            ensure!(
                graphs.get(&from).is_some_and(|found| found.contains(&to))
                    && response::Filter::source(&s, sa + 0x10)?
                        == response::Filter::native(&n, na + 0x10)?,
                "Projectile profile graph response differs"
            );
        }
    }
    let extra = nr[sr.len()..]
        .iter()
        .map(|&r| n.u32(n.pointer(r)? - 4))
        .collect::<Result<Vec<_>>>()?;
    Ok((
        fingerprint(to, &n),
        json!({"source":from,"native":to,"amount_bits":source_amount,
        "damage_type":{"source":source_amount[12],"native":native_amount[12],"private":retype.is_some()},
        "categories":{"source":from_categories,"native":to_categories,"private":recategorized},
        "extra_native_responses":extra,"source_response_bodies":bodies.len(),"source_amount":bounds.is_some(),
        "compatibility":"Push and 80802B30 response bodies use source values in a private copy. Other native response bodies, audio and feedback are retained."}),
        Private {
            damage_type: retype,
            categories: recategorized.then_some(from_categories),
            bodies,
            amount: bounds,
        },
    ))
}

/// For each source profile the source tree's responses or movement profile fields name, the
/// carrier profile a created response or an empty carrier field may take when nothing in the tree
/// pairs it: the first carrier profile that qualifies as [`profile`] checks, preferring one that
/// needs no private copy. Candidates are the carrier's direct-hit and secondary profiles, then its
/// tree's in order. Hard Light's inserted group
/// responds with `80A6A4C0`, which has its direct-hit profile `80A6A4BF`'s amounts, categories,
/// response classes and feedback and qualifies against carrier direct-hit profile `8152C7A2` as
/// `80A6A4BF` does. Native response bodies stay, as for every paired profile.
fn fallback_profiles(
    source: &Source,
    native: &mut Reader,
    (sr, nr): (&response::Responses, &response::Responses),
    (fields, movement): (&[u32], &graph::Owner),
) -> Result<BTreeMap<u32, u32>> {
    let mut carrier = vec![
        movement.payload.u32(movement.definition + 0x118)?,
        movement.payload.u32(movement.definition + 0x1D0)?,
    ];
    carrier.extend(nr.profiles());
    let mut seen = BTreeSet::new();
    carrier.retain(|&tag| !matches!(tag, 0 | u32::MAX) && seen.insert(tag));
    let none = response::Counterparts::new();
    let mut found = BTreeMap::new();
    for tag in sr.profiles().into_iter().chain(fields.iter().copied()) {
        if matches!(tag, 0 | u32::MAX) || found.contains_key(&tag) {
            continue;
        }
        let mut first = None;
        for &to in &carrier {
            match profile(source, native, (tag, to), &none) {
                Ok((_, _, private)) if private == Private::default() => {
                    first = Some(to);
                    break;
                }
                Ok(_) => {
                    first.get_or_insert(to);
                }
                Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                Err(_) => {}
            }
        }
        if let Some(to) = first {
            found.insert(tag, to);
        }
    }
    Ok(found)
}

/// Fingerprints and evidence for every paired resource, and what each carrier profile takes in its
/// private copy. One carrier profile cannot pair with source profiles that deal different types
/// or declare different categories where the carrier names it. A profile only `created` profile
/// responses name takes its own copy instead when it differs from the others on its carrier
/// profile, as the four profiles of Telesto's created responses do on carrier profile `815266E8`.
fn resources(
    source: &Source,
    native: &mut Reader,
    (pairs, created): ResourcePairs<'_>,
    graphs: &response::Counterparts,
) -> Result<RetypedResponses> {
    let mut fingerprints = Vec::new();
    let mut evidence = Vec::new();
    let mut types = BTreeMap::<u32, Private>::new();
    let mut splits = bank::Splits::new();
    let mut shared = BTreeSet::new();
    let is_created = |pair: &&(u8, u32, u32)| pair.0 == 9 && created.contains(&(pair.1, pair.2));
    let kept_first = pairs
        .iter()
        .filter(|pair| !is_created(pair))
        .chain(pairs.iter().filter(is_created));
    for pair in kept_first {
        let &(kind, from, to) = pair;
        if matches!(from, 0 | u32::MAX) || matches!(to, 0 | u32::MAX) {
            ensure!(from == to, "Projectile resource presence differs");
            continue;
        }
        if kind == 9 {
            let (hash, report, private) = profile(source, native, (from, to), graphs)?;
            let kept = types.entry(to).or_insert_with(|| private.clone());
            let same = kept.damage_type == private.damage_type
                && kept.categories == private.categories
                && kept.amount == private.amount;
            if !same && is_created(&pair) {
                splits.insert(from, (to, private));
                fingerprints.push(hash);
                evidence.push(report);
                continue;
            }
            ensure!(
                same,
                "A carrier profile pairs with source profiles of different damage types or categories ({from:08X} on {to:08X}, among {})",
                pairs
                    .iter()
                    .filter(|pair| pair.0 == 9 && pair.2 == to)
                    .map(|pair| format!("{:08X}", pair.1))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            // One carrier profile paired with source profiles of different bodies keeps its own.
            if kept.bodies != private.bodies || shared.contains(&to) {
                kept.bodies.clear();
                shared.insert(to);
            }
            fingerprints.push(hash);
            evidence.push(report);
            continue;
        }
        let (sc, nc) = match kind {
            // Spawned and attached graphs, selectors 4, 8 and Rise of Iron 13 and 15.
            4 | 8 | 13 | 15 => (0x80800734, 0x80809C0F),
            30 => (0x808005F7, 0x80808BCD),
            31 => (0x8080080A, 0x80809802),
            32 => (0x80802637, 0x80803EB0),
            _ => anyhow::bail!("Unsupported projectile resource role"),
        };
        ensure!(
            source.reference(from)? == sc,
            "Source projectile resource class differs"
        );
        let s = source.read(from)?;
        // The carrier keeps its own presentation resource in the movement's feedback field,
        // which machine gun carrier movement `81529CC9` fills with cue `80BF4620` instead.
        let nc = if kind == 32 && native.tag(to, Some(0x80809802)).is_ok() {
            0x80809802
        } else {
            nc
        };
        let n = native.tag(to, Some(nc))?;
        fingerprints.push(fingerprint(to, &n));
        evidence.push(json!({"source":from,"native":to,"source_class":sc,"native_class":nc,
            "source_sha256":hex::encode(Sha256::digest(&s.0)),"compatibility":"Native effect resource is retained."}));
    }
    let retyped = types
        .into_iter()
        .filter(|(_, private)| *private != Private::default())
        .collect();
    Ok((fingerprints, evidence, retyped, splits))
}

/// A child creation parameter only the source has can be left out when no graph the source
/// movement creates mentions its name: its selector 4, 8 and 15 graphs and its stop-child and
/// final-impact graphs at definition `+118` and `+180`, with every owner they list. Creation
/// transfer ignores names a graph does not declare. Automatic rifle source `80A7E1BD` passes
/// `9602E894`, which neither its spawned graph `80A6A699` nor the carrier declares. The inputs of a
/// left-out parameter take a `dropped/` path, so their connections are not compared.
fn omit_unread(
    source: &Source,
    movement: &graph::Owner,
    responses: &mut response::Responses,
    omitted: &[(u32, u32)],
) -> Result<()> {
    if omitted.is_empty() {
        return Ok(());
    }
    let mut graphs = responses.spawned();
    for at in [0x118, 0x180] {
        let tag = movement.payload.u32(movement.definition + at)?;
        if !matches!(tag, 0 | u32::MAX) {
            graphs.insert(tag);
        }
    }
    let mut payloads = Vec::new();
    for graph in graphs {
        let p = source.read(graph)?;
        for row in p.array(32, 12, Some(0x80800715))? {
            payloads.push(source.read(p.u32(row)?)?);
        }
        payloads.push(p);
    }
    for &(namespace, name) in omitted {
        for p in &payloads {
            for at in (0..p.0.len().saturating_sub(3)).step_by(4) {
                ensure!(
                    p.u32(at)? != name,
                    "Source child creation parameter {name:08X} may have a reader"
                );
            }
        }
        let prefix = format!("child/{namespace:08X}/{name:08X}/");
        for path in responses.inputs.values_mut() {
            if path.starts_with(&prefix) {
                *path = format!("dropped/{path}");
            }
        }
    }
    Ok(())
}

/// The prefix and root symbol of a translated spawned graph.
fn spawn_prefix(tag: u32) -> String {
    format!("projectile-spawn-{tag:08X}")
}

pub(super) fn spawn_symbol(tag: u32) -> String {
    format!("{}-root", spawn_prefix(tag))
}

/// Translate every source graph a created spawn names without a native counterpart, as the
/// victim explosion route does, and publish its assets and particles with the projectile. Its
/// cues join the weapon's audio closure.
fn translate_spawns(
    assembly: &mut Assembly<'_>,
    plan: &edit::Plan,
    assets: &mut Value,
) -> Result<Vec<Value>> {
    let start = assembly.nodes.len();
    let mut report = Vec::new();
    for tag in plan.spawns() {
        let closure = crate::d1_roi::native::perk::pattern::closure::translate_closure(
            assembly.source,
            &mut assembly.native,
            tag,
            &spawn_prefix(tag),
            false,
        )
        .with_context(|| format!("Projectile spawn graph {tag:08X}"))?;
        ensure!(
            closure.root == spawn_symbol(tag),
            "Translated projectile spawn root differs"
        );
        for asset in &closure.assets {
            let patches = asset
                .patches
                .iter()
                .map(|patch| (patch.offset, patch.symbol.clone()))
                .collect::<Vec<_>>();
            let template = *closure
                .templates
                .get(&asset.symbol)
                .context("Translated spawn asset template")?;
            assembly.node(&asset.symbol, template, &asset.payload.0, None, &patches)?;
        }
        for node in &closure.nodes {
            assembly.node(
                &node.symbol,
                node.template,
                &node.payload,
                node.reference.as_deref(),
                &node.patches,
            )?;
        }
        assembly
            .projectile_cues
            .extend(closure.cues.iter().copied());
        report.push(json!({"source":tag,"root":closure.root,"cues":closure.cues,
            "translation":closure.provenance}));
    }
    let nodes = assembly.nodes.drain(start..).collect::<Vec<_>>();
    assets["nodes"]
        .as_array_mut()
        .context("Projectile assets lack nodes")?
        .extend(nodes);
    Ok(report)
}

pub(super) fn prepare(assembly: &mut Assembly<'_>, export: &Value, donor: u32) -> Result<Value> {
    let selected = selection::source(assembly.source, export)?;
    let carriers = selection::carriers(&mut assembly.native, donor, selected.key)?;
    // The base's own carrier comes first and is the one a refusal names. A later carrier of
    // the base's content owner is tried only after it refuses, each from a clean assembly.
    let mut refusal = None;
    let mut others = Vec::new();
    for selection in carriers {
        let (from, carrier) = (selected.graph, selection.graph);
        let nodes = assembly.nodes.len();
        let evidence = assembly.evidence.len();
        let symbols = assembly.symbols.clone();
        let cues = assembly.projectile_cues.clone();
        let result = prepare_pair(assembly, selected, selection)
            .with_context(|| format!("Source projectile {from:08X} on carrier {carrier:08X}"));
        match result {
            Ok(mut report) => {
                // Why each earlier carrier of the base was passed over.
                let passed = refusal
                    .iter()
                    .map(|error| format!("{error:#}"))
                    .chain(others)
                    .collect::<Vec<_>>();
                if !passed.is_empty() {
                    report["passed_over_carriers"] = json!(passed);
                }
                return Ok(report);
            }
            Err(error) => {
                crate::cancellation::check()?;
                assembly.nodes.truncate(nodes);
                assembly.evidence.truncate(evidence);
                assembly.symbols = symbols;
                assembly.projectile_cues = cues;
                assembly.projectile_names.clear();
                if refusal.is_none() {
                    refusal = Some(error);
                } else {
                    others.push(format!("{carrier:08X} ({:#})", error.root_cause()));
                }
            }
        }
    }
    let refusal = refusal.context("Native weapon offers no projectile carrier")?;
    if others.is_empty() {
        return Err(refusal);
    }
    anyhow::bail!("{refusal:#}. Other carriers: {}", others.join(", "))
}

fn prepare_pair(
    assembly: &mut Assembly<'_>,
    selected: selection::SourceSelection,
    selection: selection::Native,
) -> Result<Value> {
    let source = assembly.source;
    let native = &mut assembly.native;
    let (key, from) = (selected.key, selected.graph);
    let s = graph::Graph::read(from, true, |tag| source.read(tag))?;
    let mut n = graph::Graph::read(selection.graph, false, |tag| {
        Ok((*native.tag(tag, None)?).clone())
    })?;
    n.leave_out_absent(&s);
    let groups = align::groups(
        s.owner(graph::Role::Movement)?,
        n.owner(graph::Role::Movement)?,
        &mut |a, b| {
            material::pairs(
                source,
                native,
                (
                    s.owner(graph::Role::Movement)?,
                    n.owner(graph::Role::Movement)?,
                ),
                (a, b),
            )
        },
    )?;
    let materials = material::prepare(
        source,
        native,
        s.owner(graph::Role::Movement)?,
        n.owner(graph::Role::Movement)?,
        &groups.paired,
    )?;
    let mut sr = response::Responses::read(s.owner(graph::Role::Movement)?, true, &[])?;
    let mut nr =
        response::Responses::read(n.owner(graph::Role::Movement)?, false, &groups.dropped)?;
    let (sm, nm) = (
        s.owner(graph::Role::Movement)?,
        n.owner(graph::Role::Movement)?,
    );
    let fields = [
        sm.payload.u32(sm.definition + 0x128)?,
        sm.payload.u32(sm.definition + 0x1E0)?,
    ];
    let fallback = fallback_profiles(source, native, (&sr, &nr), (&fields, nm))?;
    let registry = native.tag(0x80C70CA1, None)?;
    let mut translation = sr.translate(
        &mut nr,
        n.owner(graph::Role::Movement)?,
        &groups,
        (n.owner(graph::Role::Movement)?.instance, &fallback),
        &|at| crate::d1_roi::native::perk::label::inline(source, &sm.payload, at, &registry),
    )?;
    translation.plan.materials = materials.inserted.clone();
    omit_unread(
        source,
        s.owner(graph::Role::Movement)?,
        &mut sr,
        &translation.omitted,
    )?;
    let (patches, scalar_patches) = (&translation.patches, &translation.scalar_patches);
    let (launch_patches, launch) = launch::patches(
        s.owner(graph::Role::Movement)?,
        n.owner(graph::Role::Movement)?,
    )?;
    let mut resource_pairs = translation.resources.clone();
    // A fallback a created response names is qualified with the other profile pairs.
    let created = translation.plan.created_profiles();
    let mut created_pairs = BTreeSet::new();
    for (&from, &to) in &fallback {
        if response::unique(&translation.graphs, from).is_none() && created.contains(&to) {
            // A pair the tree or a profile field also holds is not a created response's alone.
            if !resource_pairs.contains(&(9, from, to)) {
                created_pairs.insert((from, to));
            }
            resource_pairs.insert((9, from, to));
        }
    }
    let additional_inputs = sr.additional_inputs(&nr);
    let variants = s.variants(
        &n,
        |tag| source.read(tag),
        |tag| Ok((*native.tag(tag, None)?).clone()),
    )?;
    let channels = s.compare(
        &n,
        &sr.inputs,
        &nr.inputs,
        &additional_inputs,
        &translation.created,
        &variants.admitted,
    )?;
    // A source profile field the carrier leaves empty takes its own private copy of the carrier
    // profile that qualifies against it, as Dreg's Promise's Arc direct hit `80AADD84` does where
    // carrier movement `8152AE80` names none. Its carrier profile may serve the tree as well.
    let mut fills = Vec::new();
    let mut fingerprints_extra = Vec::new();
    let mut fill_report = Vec::new();
    for (kind, a, b) in [
        (9, 0x128, 0x118),
        (9, 0x1E0, 0x1D0),
        (30, 0x1F0, 0x1E0),
        (31, 0x200, 0x1F0),
        (32, 0x210, 0x200),
    ] {
        let from = sm.payload.u32(sm.definition + a)?;
        let to = nm.payload.u32(nm.definition + b)?;
        if kind == 9
            && !matches!(from, 0 | u32::MAX)
            && to == u32::MAX
            && let Some(&chosen) = fallback.get(&from)
        {
            let (hash, report, private) =
                profile(source, native, (from, chosen), &translation.graphs)?;
            fingerprints_extra.push(hash);
            fill_report.push(json!({"field":b,"profile":report}));
            fills.push((b, chosen, private));
            continue;
        }
        created_pairs.remove(&(from, to));
        resource_pairs.insert((kind, from, to));
    }
    let (mut fingerprints, resource_report, retyped, splits) = resources(
        source,
        native,
        (&resource_pairs, &created_pairs),
        &translation.graphs,
    )?;
    fingerprints.append(&mut fingerprints_extra);
    fingerprints.extend(materials.fingerprints);
    let carrier_row = selection.row;
    fingerprints.extend(selection.fingerprints);
    fingerprints.push(fingerprint(selection.graph, &n.payload));
    fingerprints.extend(
        n.owners
            .values()
            .map(|owner| fingerprint(owner.tag, &owner.payload)),
    );
    fingerprints.sort_by_key(|r| r["tag"].as_u64());
    fingerprints.dedup();
    let mut assets = bank::prepare(
        assembly,
        &s,
        &n,
        selection.graph,
        (
            &[
                patches.as_slice(),
                scalar_patches.as_slice(),
                launch_patches.as_slice(),
            ]
            .concat(),
            &materials.patches,
            (&retyped, &fills, &splits),
        ),
        (&groups.paired, &groups.dropped),
        (&translation.plan, &variants.values),
    )?;
    let spawned = translate_spawns(assembly, &translation.plan, &mut assets)?;
    let root = assets["root"].clone();
    Ok(
        json!({"status":"retained_native","source_key":key,"source_graph":from,
        "source_sha256":hex::encode(Sha256::digest(&s.payload.0)),
        "native_graph":selection.graph,"native_content_owner":selection.content,
        "native_content_resource":selection.resource,"native_key":selection.key,"fingerprints":fingerprints,
        "source_selection":selected.route,"native_content_default":selection.content_default,"native_barrel":selection.barrel,
        "native_carrier_row":carrier_row,
        "patches":patches,"scalar_patches":scalar_patches,"launch_patches":launch_patches,"launch":launch,
        "material_patches":materials.patches,"material_groups":materials.groups,
        "responses":sr.report(),"tree":translation.report,"channels":channels,"resources":resource_report,
        "compatibility":["The source-selected counterpart supplies native movement, collision, effects and audio.",
            "The private movement takes the source's launch speed multiplier and distance curve, which the base Barrel's launch inputs scale.",
            "Matched response count and speed-variation operands, and root scalar constants, use source values on a private native owner.",
            "Response material references use matching source identities through checked native ancestry. Native material descendants and target filters are retained.",
            "A private channel bank keeps every source declaration and producer, followed by the carrier's other declarations for its retained consumers.",
            "Additional native child creation parameters retain their named inputs and may affect spawned entities.",
            "An unnamed settings input keeps the source selection, which Shadowkeep's binder does not store.",
            "Native contact category 10, native response event gates and additional native profile responses are retained.",
            "A carrier profile whose source deals another damage type is copied privately with the source type.",
            "Named response groups only the carrier has, and the root scalars only they read, are left out of the private movement."],
        "private_bank":true,"root":root,"assets":assets,"translated_spawns":spawned,
        "gameplay_verified":false}),
    )
}

/// Paired resources by role, source and native tag, and the profile pairs among them that only
/// created profile responses name, by source and carrier profile.
type ResourcePairs<'a> = (&'a BTreeSet<(u8, u32, u32)>, &'a BTreeSet<(u32, u32)>);

type RetypedResponses = (Vec<Value>, Vec<Value>, BTreeMap<u32, Private>, bank::Splits);
