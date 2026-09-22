//! Conversion is reviewed against the current draft, then committed without losing overrides.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action,
    activation::{self, PerkActivation},
    program::{self, Program, decompile},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::app::custom_perks) struct Input {
    index: u16,
    activation: Option<PerkActivation>,
    values: Vec<WeaponRuntimeValueOverride>,
    action_values: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    projectiles: Vec<ProjectileSelection>,
}

#[derive(Debug)]
pub(in crate::app::custom_perks) struct Preview {
    program: Program,
    fidelity: Result<Vec<decompile::Difference>, String>,
    /// Whether the stock action fit the typed program or is carried in native form.
    recovery: decompile::Recovery,
}

impl PerkEditor {
    fn conversion_input(&self) -> Input {
        Input {
            index: self.key.source_perk_index,
            activation: self.activation,
            values: self.draft.clone(),
            action_values: self.action_draft.clone(),
            projectiles: self.projectile_draft.clone(),
        }
    }

    pub(super) fn draw_conversion(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        loaded: &Arc<PrivatePerkRuntimeGraph>,
        experimental: bool,
    ) {
        if self.receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Preparing Behavior…");
            });
            return;
        }
        let input = self.conversion_input();
        let valid = self.validation_errors().is_empty() && !self.is_loading();
        if let Some((_, result)) = self
            .preview
            .as_ref()
            .filter(|(prepared, _)| *prepared == input)
        {
            match result {
                Ok(preview) => {
                    if experimental && valid && preview.fidelity.as_ref().is_ok_and(Vec::is_empty) {
                        let mut program = preview.program.clone();
                        program.name = self.plug_label.clone();
                        self.conversion = Some(program);
                        return;
                    }
                    if let decompile::Recovery::NativeForm(reason) = &preview.recovery {
                        ui.small(format!(
                            "Carried in native form because the typed program does not hold it yet. {reason}"
                        ));
                    }
                    self.conversion = behavior::draw_conversion(
                        ui,
                        Some(&Ok(preview.program.clone())),
                        &preview.fidelity,
                        &self.plug_label,
                        experimental && valid,
                    );
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    ui.small("The stock effect and its edits are preserved.");
                }
            }
            return;
        }
        if experimental && valid {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Preparing Behavior…");
            });
            let packages = self.packages.clone();
            let loaded = Arc::clone(loaded);
            let repaint = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            self.receiver = Some(receiver);
            self.worker = Some(thread::spawn(move || {
                let result = open_shadowkeep_package_manager(&packages)
                    .and_then(|manager| prepare(&manager, &loaded, &input));
                let _ = sender.send(PrivatePerkGraphEvent::Preview(input, result));
                repaint.request_repaint();
            }));
        } else if !experimental {
            ui.weak("Enable Experimental Features to edit behavior.");
        }
    }
}

fn prepare(
    manager: &sundial::package_authoring::PackageManager,
    loaded: &PrivatePerkRuntimeGraph,
    input: &Input,
) -> Result<Preview, String> {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None)?)
        .map_err(|error| error.to_string())?;
    let stock = load_sandbox_perk_runtime_action(manager, &globals, usize::from(input.index))?;
    if stock.action_payload != loaded.action_payload {
        return Err("The source action changed. Reopen the effect before converting it.".into());
    }
    let registry = if input.activation.is_some() {
        manager
            .read_tag(tiger_pkg::TagHash(activation::LABEL_GLOBALS))
            .map_err(|error| error.to_string())?
    } else {
        Vec::new()
    };
    let mut payload =
        effective_action(loaded.action_tag, &loaded.action_payload, input, &registry)?;
    let graphs = projectile::resolve(manager, &stock, &input.projectiles)?;
    for (source, effective) in stock.graphs.iter().zip(&graphs) {
        for &offset in &effective.action_offsets {
            let field = payload
                .get_mut(offset..offset + 4)
                .ok_or("A projectile reference moved.")?;
            if field != source.tag.0.to_le_bytes() {
                return Err("A projectile reference changed before conversion.".into());
            }
            field.copy_from_slice(&effective.tag.0.to_le_bytes());
        }
    }
    // Each route is one candidate: recover the program, carry the edits, compile it and compare
    // the compiled action with the effective stock action. The typed program is preferred so a
    // stock perk gets the same controls as a custom perk. The complete native form can carry
    // any action, so it stands in whenever the typed round trip is not exact.
    let typed = typed_program(loaded, input, &payload).and_then(|program| {
        let compiled = program::compile(manager, &program)
            .map_err(|error| format!("The typed program did not compile. {error}"))?;
        Ok(Candidate {
            fidelity: decompile::fidelity(&payload, &compiled.payload),
            program,
        })
    });
    select(typed, || {
        let program = native_program(
            loaded,
            input,
            &payload,
            graphs.iter().map(|graph| graph.tag.0),
        )?;
        let compiled = program::compile(manager, &program)
            .map_err(|error| format!("The native form did not compile. {error}"))?;
        Ok(Candidate {
            fidelity: decompile::native_fidelity(&payload, &compiled.payload),
            program,
        })
    })
}

/// Copy effective stock behavior through the same checked conversion as Edit Behavior.
/// The source index must remain intact until every override has been carried across.
pub(in crate::app::custom_perks) fn copy_effect(
    packages: &Path,
    effect: &WeaponSandboxPerkRuntimeRecipe,
    name: String,
) -> Result<Program, String> {
    let key = PerkEditorKey {
        socket_index: 0,
        choice_index: 0,
        source_plug_hash: 0,
        source_perk_index: effect.source_perk_index,
    };
    let loaded = load_private_perk_runtime_graph(packages, key, &effect.projectiles)?;
    let manager = open_shadowkeep_package_manager(packages)?;
    let input = Input {
        index: effect.source_perk_index,
        activation: effect.activation,
        values: effect.runtime_values.clone(),
        action_values: effect.action_float_values.clone(),
        projectiles: effect.projectiles.clone(),
    };
    let preview = prepare(&manager, &loaded, &input)?;
    match preview.fidelity {
        Ok(differences) if differences.is_empty() => {
            let mut program = preview.program;
            program.name = name;
            Ok(program)
        }
        Ok(_) => Err("This effect could not be copied without changing its behavior.".into()),
        Err(error) => Err(format!(
            "This effect could not be checked for an exact copy. {error}"
        )),
    }
}

/// One route through the round trip: the recovered program carrying the edits, and how the
/// action compiled from it compares with the effective stock action.
struct Candidate {
    program: Program,
    fidelity: Result<Vec<decompile::Difference>, String>,
}

impl Candidate {
    /// Whether the compiled action reproduces every checked native setting.
    fn exact(&self) -> bool {
        self.fidelity.as_ref().is_ok_and(Vec::is_empty)
    }

    /// Why this candidate is not exact, worded for the reader of the native form's notice.
    fn shortfall(&self) -> String {
        match &self.fidelity {
            Ok(differences) => format!(
                "The typed program differs in {} checked native setting{}.",
                differences.len(),
                if differences.len() == 1 { "" } else { "s" }
            ),
            Err(error) => {
                format!("The typed program could not be checked for exactness. {error}")
            }
        }
    }
}

/// Chooses between the typed round trip and the native one.
///
/// An exact typed program wins outright, and the native route is not tried. An exact native
/// form wins next, and its notice says why the typed program fell short. When neither is
/// exact, the typed candidate keeps its lossy or unchecked reading so the user can still
/// review the differences and decide, and the native candidate stands in only when the typed
/// route failed outright. Loss is never chosen automatically.
fn select(
    typed: Result<Candidate, String>,
    native: impl FnOnce() -> Result<Candidate, String>,
) -> Result<Preview, String> {
    let typed = match typed {
        Ok(candidate) if candidate.exact() => {
            return Ok(Preview {
                program: candidate.program,
                fidelity: candidate.fidelity,
                recovery: decompile::Recovery::Typed,
            });
        }
        typed => typed,
    };
    let shortfall = match &typed {
        Ok(candidate) => candidate.shortfall(),
        Err(reason) => reason.clone(),
    };
    match (typed, native()) {
        (_, Ok(candidate)) if candidate.exact() => Ok(Preview {
            program: candidate.program,
            fidelity: candidate.fidelity,
            recovery: decompile::Recovery::NativeForm(shortfall),
        }),
        (Ok(candidate), _) => Ok(Preview {
            program: candidate.program,
            fidelity: candidate.fidelity,
            recovery: decompile::Recovery::Typed,
        }),
        (Err(_), Ok(candidate)) => Ok(Preview {
            program: candidate.program,
            fidelity: candidate.fidelity,
            recovery: decompile::Recovery::NativeForm(shortfall),
        }),
        (Err(typed), Err(native)) => Err(format!(
            "This effect cannot be prepared as a program. Typed program: {typed} Native form: {native} Keep it as a stock behavior, or reopen the effect and try again after correcting its edits."
        )),
    }
}

/// The typed program for a stock action, with the component edits carried on its assets.
/// Fails when the action lies outside the typed model or an edit has no typed asset.
fn typed_program(
    loaded: &PrivatePerkRuntimeGraph,
    input: &Input,
    payload: &[u8],
) -> Result<Program, String> {
    let decoded = action::decode(payload)?;
    let mut program = decompile::decompile(&decoded, "Custom Effect", |tag| tag)
        .map_err(|unsupported| unsupported.0)?;
    transfer_values(loaded, &input.values, &mut program)?;
    Ok(program)
}

/// The complete native form of a stock action, which can carry any shape the game emits.
fn native_program(
    loaded: &PrivatePerkRuntimeGraph,
    input: &Input,
    payload: &[u8],
    graph_tags: impl IntoIterator<Item = u32>,
) -> Result<Program, String> {
    let mut program = Program::from_native(payload, "Custom Effect")?;
    let native = program.native.as_mut().expect("complete program");
    for graph in graph_tags {
        if !native.assets.iter().any(|asset| asset.graph == graph) {
            native.assets.push(program::Asset {
                graph,
                ..program::Asset::default()
            });
        }
    }
    transfer_values(loaded, &input.values, &mut program)?;
    Ok(program)
}

fn effective_action(
    tag: u32,
    source: &[u8],
    input: &Input,
    registry: &[u8],
) -> Result<Vec<u8>, String> {
    let mut payload = match input.activation {
        Some(activation) => activation::with_activation(tag, source, activation, registry)?,
        None => source.to_vec(),
    };
    let mut offsets = BTreeSet::new();
    for value in &input.action_values {
        let offset = actions::action_offset(&payload, value)?;
        if !offsets.insert(offset)
            || actions::source_bits(&payload, value)? != value.expected_bits
            || !f32::from_bits(value.value_bits).is_finite()
        {
            return Err(
                "An action value is stale, duplicated or invalid. Correct it before converting."
                    .into(),
            );
        }
        payload[offset..offset + 4].copy_from_slice(&value.value_bits.to_le_bytes());
    }
    Ok(payload)
}

fn transfer_values(
    loaded: &PrivatePerkRuntimeGraph,
    values: &[WeaponRuntimeValueOverride],
    program: &mut Program,
) -> Result<(), String> {
    for value in values {
        let matches = loaded
            .graphs
            .iter()
            .filter(|(_, graph)| {
                graph
                    .fields()
                    .any(|field| guided::equivalent(loaded, &field.locator, &value.locator))
            })
            .map(|(tag, _)| *tag)
            .collect::<Vec<_>>();
        let [tag] = matches.as_slice() else {
            return Err(
                "A component edit cannot be assigned to one asset. Resolve it before converting."
                    .into(),
            );
        };
        let mut copied = false;
        for asset in program.assets_mut().filter(|asset| asset.graph == *tag) {
            let mut value = value.clone();
            value.locator.graph_tag = Some(*tag);
            asset.values.push(value);
            copied = true;
        }
        if !copied {
            return Err(format!(
                "The program cannot carry component edits for asset 0x{tag:08X}. Keep this effect as a stock behavior until its native references support property edits."
            ));
        }
    }
    Ok(())
}
