//! Assemble a closed gameplay dependency set before publishing an attachment group.
use super::*;

pub struct ActionInput<'a> {
    pub reader: &'a Reader,
    pub source: u32,
    pub payload: &'a Payload,
    pub graphs: &'a BTreeMap<u32, u32>,
    pub selectors: &'a sequence::Selectors,
    pub retained_graphs: &'a BTreeSet<u32>,
    pub timed_graphs: &'a BTreeMap<u32, f32>,
}

/// The authoring crate owns the native action compiler. Its callback keeps that
/// dependency out of the package translator and returns a complete private action.
pub struct CompiledAction {
    pub payload: Payload,
    pub template: u32,
}

pub struct Gameplay {
    pub manifest: Value,
    pub roots: BTreeMap<u32, String>,
    pub retained_graphs: BTreeSet<u32>,
    pub timed_graphs: BTreeMap<u32, f32>,
    pub selectors: sequence::Selectors,
    pub dictionary: (u32, String),
}

fn particles(source: &closure::Closure, request: &Request<'_>) -> Result<Value> {
    if source.particles.is_empty() {
        return Ok(json!({"nodes":[],"systems":{}}));
    }
    let render_inputs = request.directory.join("render-inputs");
    for (packages, modern, era) in [
        (request.source_packages, true, "modern"),
        (request.native_packages, false, "native"),
    ] {
        let folder = render_inputs.join(format!("tfx-{era}"));
        std::fs::create_dir_all(&folder)?;
        let mut reader = Reader::new(packages, &folder, modern)?;
        write_json(
            &folder.join("context.json"),
            &crate::tiger::shader::globals::read(&mut reader, modern)?,
        )?;
        reader.finish()?;
    }
    let decompiler = super::super::automatic::decompiler(&mut |_| {})?;
    let result =
        crate::d2_mot::particles::native::convert(&crate::d2_mot::particles::native::Request {
            source_packages: request.source_packages,
            native_packages: request.native_packages,
            render_inputs: &render_inputs,
            decompiler: &decompiler,
            graph: request.directory,
            work: &request.directory.join("particle-conversion"),
            systems: &source.particles.iter().copied().collect::<Vec<_>>(),
            inputs: &BTreeMap::new(),
            template: 0x80EF30DB,
        })?;
    Ok(result)
}

fn identity(
    bindings: &mut sequence::Bindings,
    namespace: &str,
    owner: u32,
    key: u32,
) -> Result<()> {
    if bindings.identities.contains_key(&key) {
        return Ok(());
    }
    let value = format!("{namespace}/attachment/{owner:08X}/{key:08X}")
        .bytes()
        .fold(0x811C9DC5u32, |hash, byte| {
            hash.wrapping_mul(16777619) ^ u32::from(byte)
        });
    ensure!(
        !matches!(value, 0 | u32::MAX | 0x811C9DC5)
            && !bindings
                .identities
                .values()
                .any(|&previous| previous == value),
        "private attachment identity is reserved or collides"
    );
    bindings.identities.insert(key, value);
    Ok(())
}

fn sequences(
    native: &mut Reader,
    assets: &mut Assets,
    closure: &closure::Closure,
    resources: &BTreeMap<u32, BTreeMap<usize, sequence::Resource>>,
    particles: &Value,
    namespace: &str,
    presentation: &presentation::Plan,
) -> Result<BTreeMap<u32, sequence::Bindings>> {
    let templates = [0x80BC3A79, 0x80BC3A6C, 0x80B82848, 0x80BEA4CB]
        .into_iter()
        .map(|tag| native.tag(tag, None).map(|p| (*p).clone()))
        .collect::<Result<Vec<_>>>()?;
    let mut provider_metadata = BTreeMap::new();
    metadata(
        native,
        &templates.iter().collect::<Vec<_>>(),
        &mut provider_metadata,
    )?;
    let mut output = BTreeMap::new();
    for (&tag, owner) in &closure.owners {
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? != 0x80808179 {
            continue;
        }
        let mut bindings = sequence::Bindings {
            resources: resources.get(&tag).cloned().unwrap_or_default(),
            event_templates: templates.clone(),
            metadata: provider_metadata.clone(),
            omitted_events: presentation.events.get(&tag).cloned().unwrap_or_default(),
            omitted_query_actions: presentation
                .query_actions
                .get(&tag)
                .cloned()
                .unwrap_or_default(),
            ..Default::default()
        };
        identity(&mut bindings, namespace, tag, owner.u32(d + 0x30C)?)?;
        for row in owner.array(d + 0x1D8, 24, Some(0x808091F1))? {
            let event = owner.pointer(row + 16)?;
            if bindings.omitted_events.contains(&event) {
                continue;
            }
            if owner.u32(event - 4)? != 0x808067B9 {
                continue;
            }
            for row in owner.array(event + 0x28, 24, Some(0x808067BB))? {
                for field in sequence::presentation::fields(owner, row)? {
                    let source = owner.u32(field)?;
                    let symbol = particles["systems"][format!("{source:08X}")]
                        .as_str()
                        .context("attachment sequence particle has no converted symbol")?;
                    bindings.resources.insert(
                        field,
                        sequence::Resource {
                            source_class: 0x80806920,
                            native_class: 0x80806E28,
                            tag: assets.external(native, symbol)?,
                        },
                    );
                }
            }
            identity(&mut bindings, namespace, tag, owner.u32(event + 0x108)?)?;
        }
        output.insert(tag, bindings);
    }
    Ok(output)
}

/// `published` contains names established by the caller's native parent context.
/// Other names require the qualified numeric context lookup, whose absent-name
/// behavior is zero in both builds. The conversion does not invent publications.
pub fn convert_gameplay(
    source: &mut Reader,
    native: &mut Reader,
    request: &Request<'_>,
    roots: &[u32],
    parameters: &BTreeMap<u32, BTreeMap<u32, [u8; 16]>>,
    published: &BTreeSet<u32>,
    mut compile: impl FnMut(ActionInput<'_>) -> Result<CompiledAction>,
) -> Result<Gameplay> {
    let mut dependencies = roots.to_vec();
    dependencies.push(request.source_tag);
    let mut closure = closure::read(source, &dependencies)?;
    ensure!(
        !closure.graphs.is_empty(),
        "gameplay attachment closure has no graphs"
    );
    for (&graph, values) in parameters {
        ensure!(
            roots.contains(&graph),
            "creation parameters are outside the requested roots"
        );
        if let Some(previous) = closure.parameters.insert(graph, values.clone()) {
            ensure!(
                previous == *values,
                "root and nested callers supply differing attachment parameters"
            );
        }
    }
    let specialized = lifetime::parameters(&closure)?;
    let retained_graphs = lifetime::sequence_retention(&closure)?;
    let timed_graphs = lifetime::timer_retention(&closure)?;
    let mut presentation = presentation::Plan::read(source, native, &closure)?;
    let mut assets = Assets::default();
    let plans = graph::Plan::reserve(source, native, &closure, &mut assets)?;
    let graph_tags = plans
        .iter()
        .map(|(&source, graph)| (source, graph.target))
        .collect::<BTreeMap<_, _>>();
    let mut resources = resources::Plan::reserve(native, &closure, &mut assets, &plans)?;
    let (dictionary, namespace, mut selectors, required) =
        namespace::prepare(source, native, &closure, &resources.tags)?;
    ensure!(
        required.is_subset(published),
        "attachment selector requires unpublished native parent inputs: {:?}",
        required.difference(published).collect::<Vec<_>>()
    );
    selectors.published_names = published.clone();
    let mut actions = BTreeMap::new();
    for owner in closure.owners.values() {
        let d = owner.pointer(24)?;
        if owner.u32(d - 4)? == 0x8080313F {
            let tag = source.ref64(owner, d + 0x50)?;
            if actions.contains_key(&tag) {
                continue;
            }
            let target = assets.reserve(native, format!("attachment-action-{tag:08X}"))?;
            let result = compile(ActionInput {
                reader: source,
                source: tag,
                payload: closure
                    .actions
                    .get(&tag)
                    .context("hosted action is outside closure")?,
                graphs: &graph_tags,
                selectors: &selectors,
                retained_graphs: &retained_graphs,
                timed_graphs: &timed_graphs,
            })?;
            ensure!(
                native.reference(result.template)? == 0x808040B5,
                "compiled attachment action template has the wrong native class"
            );
            assets.push(target, result.template, result.payload, None)?;
            actions.insert(tag, target);
            resources.bind(tag, target, 0x808040B5)?;
        }
    }
    let particles = particles(&closure, request)?;
    presentation.particles(&closure, &particles)?;
    let sound_template = native.tag(0x80F2E50A, None)?;
    let sound = (0..sound_template.0.len().saturating_sub(3))
        .step_by(4)
        .filter_map(|at| sound_template.u32(at).ok())
        .find(|&tag| native.reference(tag).ok() == Some(0x80809802))
        .context("native attachment sound envelope")?;
    let skipped = audio::unsupported(source, &closure.sounds)?;
    presentation.sounds(source, &closure, &skipped)?;
    resources.omit_sounds(&skipped);
    let supported = closure
        .sounds
        .iter()
        .copied()
        .filter(|tag| !skipped.contains_key(tag))
        .collect();
    let (sounds, audio) = audio::convert(source, native, &mut assets, &supported, sound, request)?;
    for (source, target) in sounds {
        resources.bind(source, target, 0x80809802)?;
    }
    let resource_report = resources.emit(
        native,
        &closure,
        &mut assets,
        dictionary,
        &namespace,
        &selectors,
    )?;
    let fields = resources.bindings(&closure)?;
    let sequences = sequences(
        native,
        &mut assets,
        &closure,
        &fields,
        &particles,
        request.namespace,
        &presentation,
    )?;
    let bindings = graph::Bindings {
        resources: &fields,
        sequences: &sequences,
        selectors: &selectors,
        specialized: &specialized,
        actions: &actions,
    };
    let mut graphs = Vec::new();
    for plan in plans.into_values() {
        graphs.push(plan.emit(
            source,
            native,
            &closure,
            &mut assets,
            &bindings,
            |graph, inputs| lifetime::stable(&closure, graph, inputs, &specialized),
        )?);
    }
    let exported = graph_tags
        .iter()
        .map(|(&source, target)| {
            Ok((
                source,
                assets
                    .names
                    .get(target)
                    .context("missing emitted attachment symbol")?
                    .clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let root = *graph_tags
        .values()
        .next()
        .context("attachment graph root")?;
    let dictionary_symbol = assets
        .names
        .get(&resources.tags[&dictionary])
        .context("private category dictionary has no symbol")?
        .clone();
    let mut attachment = assets.write(
        request.directory,
        root,
        json!({"sources":roots,"graphs":graphs,"resources":resource_report,"audio":audio,
            "sequence_retained_graphs":retained_graphs,
            "timer_retained_graphs_seconds":timed_graphs,
            "presentation_differences":presentation.report,
            "optional_numeric_context_names":selectors.optional_names,
            "category_query_aliases":namespace.query_aliases,
            "published_context_names":published,"gameplay_verified":false}),
    )?;
    attachment["roots"] = json!(exported.values().collect::<Vec<_>>());
    attachment
        .as_object_mut()
        .context("attachment manifest")?
        .remove("root");
    Ok(Gameplay {
        manifest: json!({"particles":particles,"attachments":attachment}),
        roots: exported,
        retained_graphs,
        timed_graphs,
        selectors,
        dictionary: (dictionary, dictionary_symbol),
    })
}
