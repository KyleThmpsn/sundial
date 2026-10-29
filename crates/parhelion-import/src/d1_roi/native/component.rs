//! Native component assembly retains source activation and named material inputs.
use super::*;
use crate::tiger::{
    entity::{compiled, links::NativeRows},
    instance,
};

mod channel;
mod light;
mod optic;
mod query;
mod sequence;

/// A stock weapon part entity: render component, markers and channel bank, each bound to the
/// weapon's skeleton provider `AE7D0AED`. The character templates below bind to the character
/// skeleton (`81F7D33C`), which a weapon runtime never publishes, so a weapon part built on them
/// receives no bones and draws in no view.
const WEAPON_PART_ENTITY: u32 = 0x81522FE2;
const WEAPON_RENDER: u32 = 0x81522FE0;
const WEAPON_MARKERS: u32 = 0x81522FE1;
const CHARACTER_PART_ENTITY: u32 = 0x80BC358D;
const CHARACTER_RENDER: u32 = 0x80BC359E;

struct Component {
    id: u32,
    template_tag: u32,
    template: Payload,
    entity: Payload,
    payload: Payload,
    symbol: String,
    patches: Vec<Value>,
    inputs: Vec<(u32, usize)>,
    /// Source owner and input definition paired with the emitted getter definition.
    incoming: Vec<(u32, usize, usize)>,
}

impl Component {
    fn patch(&mut self, at: usize, symbol: &str) -> Result<()> {
        put(&mut self.payload.0, at, &u32::MAX.to_le_bytes())?;
        self.patches.push(json!({"offset":at,"symbol":symbol}));
        Ok(())
    }

    fn seal(&mut self) -> Result<()> {
        let end = self.payload.0.len();
        instance::seal_span(&mut self.payload, &mut self.patches, end)?;
        size(&mut self.payload.0)
    }
}

fn replace_owner(p: &mut Payload, before: u32, after: u32) -> Result<()> {
    for at in (0..p.0.len().saturating_sub(15)).step_by(4) {
        if p.u32(at)? == before {
            ensure!(
                p.u32(at + 4)? & 0xFFFF0000 == 0x80800000 && p.u64(at + 8)? < p.0.len() as u64,
                "Untyped native component owner reference"
            );
            put(&mut p.0, at, &after.to_le_bytes())?;
        }
    }
    Ok(())
}

fn activation(source: &Payload, target: &mut Payload) -> Result<()> {
    let from = source.pointer(24)?;
    let to = target.pointer(24)?;
    let rows = source.array(from + 0x50, 24, Some(0x80800785))?;
    let mut groups = Vec::new();
    let mut conditions = Vec::new();
    for row in rows {
        let mut bytes = source.bytes::<24>(row)?;
        bytes[8..].fill(0);
        groups.extend(bytes);
        let mut terms = Vec::new();
        for term in source.array(row + 8, 8, Some(0x8080055F))? {
            // Both selector names and every enumerated value are shared by the
            // captured D1 and Shadowkeep state-selector components.
            ensure!(
                matches!(
                    (source.u32(term)?, source.u32(term + 4)?),
                    (0x69EA00AA, 0xB976E27E | 0x83651CD5) | (0x090E39A4, 0x9229B22A | 0xB602B441)
                ),
                "Untranslated source armor activation term"
            );
            terms.extend(source.bytes::<8>(term)?);
        }
        conditions.push(terms);
    }
    array(&mut target.0, to + 0x38, 0x80809C2C, &groups, 24)?;
    for (row, terms) in target
        .array(to + 0x38, 24, Some(0x80809C2C))?
        .into_iter()
        .zip(conditions)
    {
        array(&mut target.0, row + 8, 0x80809C31, &terms, 8)?;
    }
    Ok(())
}

impl Assembly<'_> {
    fn component(
        &mut self,
        template_tag: u32,
        entity_tag: u32,
        symbol: &str,
        index: usize,
    ) -> Result<Component> {
        let template = (*self.native.tag(template_tag, Some(0x80809C36))?).clone();
        let mut payload = template.clone();
        let id = 0x7E00_0000u32
            .checked_add(u32::try_from(index)?)
            .context("Temporary component identity overflow")?;
        replace_owner(&mut payload, template_tag, id)?;
        Ok(Component {
            id,
            template_tag,
            template,
            payload,
            entity: (*self.native.tag(entity_tag, Some(0x80809C0F))?).clone(),
            symbol: symbol.to_owned(),
            patches: Vec::new(),
            inputs: Vec::new(),
            incoming: Vec::new(),
        })
    }

    fn inputs(&mut self, source: &Payload, component: &mut Component) -> Result<()> {
        let si = source.pointer(16)?;
        let sr = source.pointer(24)?;
        let inputs = source.array(si + 0x118, 88, Some(0x8080077D))?;
        let definitions = source.array(sr + 0x288, 48, Some(0x80800927))?;
        ensure!(
            inputs.len() == definitions.len(),
            "Source material input count differs"
        );
        let old = &component.template;
        let ni = old.pointer(16)?;
        let parent = ni + 0x100;
        let definition = usize::try_from(old.u64(parent + 8)?)? + 0x48;
        let old_inputs = old.array(ni + 0x120, 96, Some(0x80809788))?;
        let old_definitions = old.array(definition, 40, Some(0x80809789))?;
        let ti = *old_inputs
            .first()
            .context("Native material input template absent")?;
        let td = *old_definitions
            .first()
            .context("Native material input definition absent")?;
        let mut names = Vec::new();
        for (input, def) in inputs.into_iter().zip(definitions) {
            ensure!(
                source.u64(input + 8)? == def as u64
                    && source.u64(def + 8)? == input as u64
                    && source.u64(def + 32)? == 0x808003E8,
                "Source material input reciprocal definition differs"
            );
            names.push(source.u32(def + 40)?);
        }
        let input_rows = old.0[ti..ti + 96].repeat(names.len());
        let definition_rows = old.0[td..td + 40].repeat(names.len());
        array(
            &mut component.payload.0,
            ni + 0x120,
            0x80809788,
            &input_rows,
            96,
        )?;
        array(
            &mut component.payload.0,
            definition,
            0x80809789,
            &definition_rows,
            40,
        )?;
        let inputs = component.payload.array(ni + 0x120, 96, Some(0x80809788))?;
        let definitions = component.payload.array(definition, 40, Some(0x80809789))?;
        for ((input, def), name) in inputs.into_iter().zip(definitions).zip(names) {
            put(&mut component.payload.0, input, &component.id.to_le_bytes())?;
            put(
                &mut component.payload.0,
                input + 8,
                &(def as u64).to_le_bytes(),
            )?;
            put(
                &mut component.payload.0,
                input + 16,
                &(parent as i64 - (input + 16) as i64).to_le_bytes(),
            )?;
            put(&mut component.payload.0, def, &component.id.to_le_bytes())?;
            put(
                &mut component.payload.0,
                def + 8,
                &(input as u64).to_le_bytes(),
            )?;
            put(&mut component.payload.0, def + 32, &name.to_le_bytes())?;
            component.inputs.push((name, def));
        }
        let tag = component.payload.u32(0x44)?;
        let mut allocation = (*self.native.tag(tag, None)?).clone();
        ensure!(
            crate::presentation::channel::input_allocation(
                &mut allocation,
                0x20,
                old_inputs.len(),
                component.inputs.len()
            )? == 1,
            "Native material input allocation absent or ambiguous"
        );
        let symbol = format!("{}-allocation", component.symbol);
        self.node(&symbol, tag, &allocation.0, None, &[])?;
        component.patch(0x44, &symbol)
    }

    #[allow(clippy::cognitive_complexity)]
    pub(super) fn body(&mut self, body: &Value, dyes: &[Value], symbol: &str) -> Result<String> {
        let source_entity_tag =
            u32::try_from(body["entity"].as_u64().context("Source body entity")?)?;
        let source_entity = self.source.read(source_entity_tag)?;
        let mut components = Vec::new();
        let mut source_owners = BTreeMap::new();
        let mut bank = None;
        // Connection rows name a component by tag and by its ordinal in this list, which
        // distinguishes repeated instances of one component.
        let rows = source_entity.array(0x20, 12, Some(0x80800715))?;
        for (ordinal, row) in rows.into_iter().enumerate() {
            let tag = source_entity.u32(row)?;
            let source = self.source.read(tag)?;
            let sr = source.pointer(24)?;
            let class = source.u32(sr - 4)?;
            let name = format!("{symbol}-component-{}", components.len());
            match class {
                0x80801A9C | 0x80801BF6 => {
                    let cloth = class == 0x80801BF6;
                    let source_model_tag = source.u32(sr + 0x1C4)?;
                    let source_model = self.source.read(source_model_tag)?;
                    let meshes = source_model.array(0x10, 0xA0, Some(0x80801BBF))?;
                    ensure!(!meshes.is_empty(), "Source model has no meshes");
                    ensure!(!cloth || meshes.len() == 1, "Multi-mesh source cloth requires an explicit buffer binding");
                    let equipment = self.equipment(&source, dyes)?;
                    // A material's object inputs index its drawing component's own inputs, which
                    // connect to the body's channel bank by name.
                    self.object_channels = source
                        .array(sr + 0x288, 48, Some(0x80800927))?
                        .into_iter()
                        .map(|definition| source.u32(definition + 40))
                        .collect::<Result<_>>()?;
                    // Native character setup has a fixed array of render components.
                    // Keep meshes with the same model transform in one component,
                    // including their separate layouts, buffers and draw stages.
                    let mut groups: Vec<([u8; 48], Vec<usize>)> = Vec::new();
                    for (ordinal, &mesh) in meshes.iter().enumerate() {
                        let transform = source_model.bytes::<48>(mesh)?;
                        if let Some((_, ordinals)) = groups.iter_mut().find(|(key, _)| *key == transform) {
                            ordinals.push(ordinal);
                        } else {
                            groups.push((transform, vec![ordinal]));
                        }
                    }
                    for (_, ordinals) in groups {
                        let ordinal = ordinals[0];
                        let mesh = meshes[ordinal];
                        let name = format!("{name}-mesh-{ordinal}");
                        let model = self.model(source_model_tag, &ordinals, &format!("{name}-model"), &equipment, self.weapon, cloth.then_some((&source, sr)))?;
                        ensure!(!(cloth && self.weapon), "Source weapon cloth has no native weapon-part template");
                        let (template, entity) = if cloth {
                            (0x80EF8694, 0x80EF8695)
                        } else if self.weapon {
                            (WEAPON_RENDER, WEAPON_PART_ENTITY)
                        } else {
                            (CHARACTER_RENDER, CHARACTER_PART_ENTITY)
                        };
                        let mut component = self.component(template, entity, &name, components.len())?;
                        activation(&source, &mut component.payload)?;
                        self.inputs(&source, &mut component)?;
                        let nr = component.payload.pointer(24)?;
                        component.patch(nr + 0x1DC, &model.symbol)?;
                        // Materials bind the completed source atlases directly.
                        // Native inspection still follows this optional runtime
                        // atlas root, so the donor's root must be absent before
                        // sealing copies the definition for the loader.
                        put(&mut component.payload.0, nr + 0x248, &u32::MAX.to_le_bytes())?;
                        if cloth {
                            let definition_tag = component.template.u32(nr + 0x358)?;
                            let definition = self.native.tag(definition_tag, Some(0x8080727A))?;
                            let solver_template = definition.u32(0x690)?;
                            let solver_tag = source.u32(sr + 0x8C0)?;
                            self.source.cloth_solver(&source, sr, &mut BTreeMap::new())?;
                            let solver = self.source.read(solver_tag)?;
                            let bindings = super::super::cloth::native_bindings(&source, sr, &solver.0, self.palette)?;
                            self.evidence.push(json!({"source_cloth":solver_tag,"native_structure":bindings}));
                            let solver_symbol = format!("{name}-solver");
                            self.node(&solver_symbol, solver_template, &solver.0, None, &[])?;
                            let definition_bytes = super::super::cloth::native_definition(&source, sr, &source_model, mesh, model.vertices, &definition)?;
                            let definition_symbol = format!("{name}-cloth");
                            self.node(&definition_symbol, definition_tag, &definition_bytes, None, &[(0x690, solver_symbol)])?;
                            component.patch(nr + 0x358, &definition_symbol)?;
                        }
                        component.seal()?;
                        components.push(component);
                    }
                }
                0x808008F6 => {
                    ensure!(bank.is_none(), "Source body contains multiple inherited banks");
                    let (component, names) = self.channel_bank(&source, &name, components.len())?;
                    bank = Some((components.len(), names));
                    components.push(component);
                }
                0x80802818 => {
                    let component = self.component(0x80C1D1A4, 0x80BC358D, &name, components.len())?;
                    // The selector publishes the shared gender and helmet-state values.
                    let selectors = source.array(sr + 0xC0, 24, Some(0x80802423))?;
                    ensure!(selectors.len() == 2 && source.u32(selectors[0] + 4)? == 0x69EA00AA && source.u32(selectors[1] + 4)? == 0x090E39A4, "Source state-selector contract differs");
                    let nr = component.payload.pointer(24)?;
                    let native = component.payload.array(nr + 0xA8, 24, Some(0x808038AA))?;
                    ensure!(native.len() == selectors.len(), "Native state-selector count differs");
                    for (from, to) in selectors.into_iter().zip(native) {
                        let values = source.array(from + 8, 4, Some(0x80800070))?;
                        let expected = component.payload.array(to + 8, 4, Some(0x80800070))?;
                        ensure!(source.u32(from + 4)? == component.payload.u32(to + 4)? && values.len() == expected.len()
                            && values.iter().zip(expected).all(|(a,b)| source.u32(*a).ok() == component.payload.u32(b).ok()), "Source selector values differ from the native state bank");
                    }
                    components.push(component);
                }
                0x8080242E => {
                    let mut component = self.component(0x8152FE0C, 0x80BC358D, &name, components.len())?;
                    activation(&source, &mut component.payload)?;
                    let nr = component.payload.pointer(24)?;
                    let markers = source.array(sr + 0xD8, 64, Some(0x80800608))?;
                    if self.bone_map.is_some() {
                        ensure!(markers.iter().all(|&at| source.u32(at).ok() == Some(0)
                            && source.u32(at + 4).is_ok_and(|mode| mode <= 1)),
                            "Mapped weapon markers require root bindings");
                    }
                    let rows = markers.into_iter().flat_map(|at| source.0[at..at + 64].iter().copied()).collect::<Vec<_>>();
                    array(&mut component.payload.0, nr + 0xA8, 0x80809C00, &rows, 64)?;
                    component.seal()?;
                    components.push(component);
                }
                0x808008FB if self.weapon => {
                    let layout = crate::tiger::markers::Layout {
                        row_class: 0x80800428,
                        ..crate::tiger::markers::NATIVE
                    };
                    let markers = crate::tiger::markers::read(&source, sr + 0xD8, layout)?;
                    if self.bone_map.is_some() {
                        ensure!(markers.iter().all(|m| m.binding[0] == 0 && m.binding[1] <= 1),
                            "Mapped weapon markers require root bindings");
                    }
                    let mut component = self.component(WEAPON_MARKERS, WEAPON_PART_ENTITY, &name, components.len())?;
                    activation(&source, &mut component.payload)?;
                    component.payload = crate::tiger::markers::replace(&component.payload, &markers)?;
                    component.seal()?;
                    components.push(component);
                }
                0x808027FC if self.weapon => {
                    source_owners.insert(ordinal, components.len());
                    components.push(self.optic(tag, &source, &source_entity, &name, components.len())?);
                }
                0x80803136 if self.weapon => {
                    components.push(self.query(&source, &name, components.len())?);
                }
                0x80801B6F if self.weapon => {
                    components.push(self.light(tag, ordinal, &source, &source_entity, &name, components.len())?);
                }
                0x80802733 | 0x8080203D if self.weapon => {
                    self.evidence.push(json!({"source_component":tag,"resource_class":class,
                        "omitted":"Source auxiliary sight effects require separate runtime translation"}));
                }
                0x80800610 if self.weapon && source.u64(sr + 0x190)? != 0 => {
                    source_owners.insert(ordinal, components.len());
                    components.push(self.scalar_sequence(tag, ordinal, &source, &source_entity, &name, components.len())?);
                }
                0x80800610 => self.evidence.push(json!({"source_component":tag,"resource_class":class,"omitted":"source effect sequence is outside the translated visual graph"})),
                _ => anyhow::bail!("Untranslated source body component {tag:08X}, class {class:08X}"),
            }
        }
        ensure!(
            self.weapon || bank.is_some(),
            "Source body has no inherited material bank"
        );
        let mut connections =
            self.channel_connections(&source_entity, &components, &source_owners)?;
        for (index, component) in components.iter().enumerate() {
            for at in component.entity.array(0x20, 72, Some(0x80809BC9))? {
                if component.entity.u32(at + 8)? == component.template_tag
                    && matches!(component.entity.u32(at + 12)?, 0x80809BD9 | 0x80809BD7)
                {
                    ensure!(
                        component.entity.u32(at + 40)? == u32::MAX
                            || (component.template_tag == 0x81529E1F
                                && component.entity.u64(at + 56)? == 0x20000),
                        "Native bone callback is not external or a checked optic adapter"
                    );
                    let mut row = component.entity.bytes::<72>(at)?.to_vec();
                    put(&mut row, 8, &component.id.to_le_bytes())?;
                    put(&mut row, 24, &(index as u64).to_le_bytes())?;
                    connections.push(row);
                }
            }
            for (name, def) in &component.inputs {
                let (bank_index, names) = bank
                    .as_ref()
                    .context("Source material input has no inherited bank")?;
                let ordinal = names
                    .iter()
                    .position(|n| n == name)
                    .context("Source material input is absent from its inherited bank")?;
                let carrier = &component.entity;
                let at = carrier
                    .array(0x20, 72, Some(0x80809BC9))?
                    .into_iter()
                    .find(|at| carrier.u32(at + 12).ok() == Some(0x80809789))
                    .context("Native material connection template absent")?;
                let mut row = carrier.bytes::<72>(at)?.to_vec();
                put(&mut row, 8, &component.id.to_le_bytes())?;
                put(&mut row, 16, &(*def as u64).to_le_bytes())?;
                put(&mut row, 24, &(index as u64).to_le_bytes())?;
                put(&mut row, 40, &components[*bank_index].id.to_le_bytes())?;
                let bank_payload = &components[*bank_index].payload;
                let bank_resource =
                    usize::try_from(bank_payload.u64(bank_payload.pointer(16)? + 8)?)?;
                put(&mut row, 48, &((bank_resource + 0x60) as u64).to_le_bytes())?;
                put(&mut row, 56, &(*bank_index as u64).to_le_bytes())?;
                put(&mut row, 64, &(ordinal as u32).to_le_bytes())?;
                connections.push(row);
            }
        }
        let entity_template_tag = if self.weapon {
            WEAPON_PART_ENTITY
        } else {
            CHARACTER_PART_ENTITY
        };
        let template = self.native.tag(entity_template_tag, Some(0x80809C0F))?;
        let parts = components
            .iter()
            .map(|c| compiled::Component {
                owner: c.id,
                payload: &c.payload,
                template: &c.template,
                entity: &c.entity,
            })
            .collect::<Vec<_>>();
        let mut entity = compiled::emit(
            &template,
            &parts,
            &NativeRows {
                connections,
                named_connections: Vec::new(),
            },
        )?;
        let mut entity_patches = Vec::new();
        for component in &mut components {
            for at in (0..entity.0.len().saturating_sub(3)).step_by(4) {
                if entity.u32(at)? == component.id {
                    put(&mut entity.0, at, &u32::MAX.to_le_bytes())?;
                    entity_patches.push((at, component.symbol.clone()));
                }
            }
            for at in (0..component.payload.0.len().saturating_sub(15)).step_by(4) {
                if component.payload.u32(at)? == component.id {
                    component.patch(at, &component.symbol.clone())?;
                }
            }
            let patches = component
                .patches
                .iter()
                .map(|p| {
                    Ok((
                        usize::try_from(
                            p["offset"]
                                .as_u64()
                                .context("Component relocation offset")?,
                        )?,
                        p["symbol"]
                            .as_str()
                            .context("Component relocation symbol")?
                            .to_owned(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            self.node(
                &component.symbol,
                component.template_tag,
                &component.payload.0,
                None,
                &patches,
            )?;
        }
        let entity_symbol = format!("{symbol}-entity");
        self.node(
            &entity_symbol,
            entity_template_tag,
            &entity.0,
            None,
            &entity_patches,
        )?;
        let parent_template = 0x80EC272A;
        let mut parent = (*self.native.tag(parent_template, None)?).clone();
        ensure!(parent.0.len() == 24, "Native entity parent extent differs");
        put(&mut parent.0, 16, &u32::MAX.to_le_bytes())?;
        self.node(
            symbol,
            parent_template,
            &parent.0,
            None,
            &[(16, entity_symbol)],
        )?;
        let companion_template = 0x81A662DE;
        let companion = self.native.tag(companion_template, None)?;
        self.node(
            &format!("{symbol}-companion"),
            companion_template,
            &companion.0,
            None,
            &[],
        )?;
        let node = self.nodes.last_mut().context("Body loading companion")?;
        node["shared_owner"] = json!(symbol);
        node["source_parent"] = json!(parent_template);
        Ok(symbol.to_owned())
    }
}
