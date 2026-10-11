//! Attach translated cloth to independent native float models and components.
use super::*;
use crate::d2_mot::{cloth as solver, reader::Reader};
mod activation;
mod component;
mod definition;
mod render;

// These native assets provide record layouts and callback registrations. Source
// topology, states, geometry, materials and solver data are supplied separately.
const OWNER: u32 = 0x80EF8694;
const ENTITY: u32 = 0x80EF8695;

fn patches(g: &Graph, name: &str) -> Result<Vec<Value>> {
    g.node(name)?["patches"]
        .as_array()
        .cloned()
        .context("Cloth graph relocations")
}

fn patch(rows: &mut Vec<Value>, at: usize, symbol: &str) {
    rows.retain(|p| p["offset"].as_u64() != Some(at as u64));
    rows.push(json!({"offset":at,"symbol":symbol}));
}

pub(super) fn build(
    c: &mut Effect,
    prepared: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    let models = c.source.report["models"]
        .as_array()
        .context("Cloth source models")?
        .iter()
        .enumerate()
        .filter(|(_, m)| m["cloth"] == true)
        .map(|(i, m)| (i, m.clone()))
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Ok(());
    }
    ensure!(
        c.source.report["independent_art_entity"].is_string(),
        "Cloth requires an independently converted art entity"
    );
    let manifest = load(&prepared.join("native/source-manifest.json"))?;
    let packages = Path::new(
        manifest["packages"]
            .as_str()
            .context("Native cloth template packages")?,
    );
    let mut reader = Reader::discovery(packages, &c.out.join("cloth-native"), false)?;
    let template = reader.tag(OWNER, Some(0x80809C36))?;
    let resource = template.pointer(24)?;
    let instance = template.pointer(16)?;
    ensure!(
        template.u32(instance - 4)? == 0x80807273 && template.u32(resource - 4)? == 0x80807286,
        "Native cloth component template differs"
    );
    let model_tag = template.u32(resource + 0x1DC)?;
    let model_template = reader.tag(model_tag, Some(0x808073A5))?;
    let definition_tag = template.u32(resource + 0x358)?;
    let definition_template = reader.tag(definition_tag, Some(0x8080727A))?;
    let solver_tag = definition_template.u32(0x690)?;
    let native_solver = reader.tag(solver_tag, None)?;
    ensure!(
        native_solver.u32(0)? == 0x57E0E057 && native_solver.u32(4)? == 0x10C0C010,
        "Native cloth storage template differs"
    );
    let bones = c.graph.manifest["rig_mapping"]["bone_map"]
        .as_array()
        .context("Cloth requires a compatible bone palette")?
        .iter()
        .map(|b| Ok(u16::try_from(number(b)?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut evidence = Vec::new();
    let mut converted_models = std::collections::BTreeSet::new();
    for (index, entry) in models {
        progress(format!("Converting Cloth Model {}...", index + 1));
        crate::cancellation::check()?;
        let name = format!("cloth-{index}");
        let source_tag = entry["model"].as_str().context("Cloth source model")?;
        ensure!(
            converted_models.insert(source_tag.to_owned()),
            "Multiple meshes in one cloth model need separate buffer binding validation"
        );
        let model = c.source.raw(source_tag)?;
        let source_owner = c
            .source
            .raw(entry["owner"].as_str().context("Cloth source owner")?)?;
        ensure!(
            model.array(16, 128, Some(0x80806EC5))?.len() == 1,
            "Multi-mesh cloth requires separate binding validation"
        );
        let mesh = geometry::selected_mesh(&model, &entry)?;
        let streams = geometry::streams(&c.source.root, &c.source.manifest, &model, mesh)?;
        let positions = streams
            .float_positions
            .context("Cloth must retain its float vertex stream")?
            .0;
        let vertices = positions.len() / 48;
        let wrapper_tag = entry["cloth_simulation"]["definition"]
            .as_str()
            .context("Cloth definition was not exported")?;
        let wrapper = c.source.raw(wrapper_tag)?;
        let source_solver_tag = wrapper.u32(0x6A0)?;
        let translated = solver::translate(
            &c.source.raw(&format!("{source_solver_tag:08X}"))?.0,
            Some(&bones),
        )?;
        let definition = definition::build(
            &wrapper,
            &translated.report["bindings"],
            &model,
            mesh,
            vertices,
            &definition_template,
        )?;
        c.graph.add(
            &format!("{name}-solver"),
            u64::from(solver_tag),
            &translated.bytes,
            None,
            vec![],
        )?;
        c.graph.add(
            &format!("{name}-definition"),
            u64::from(definition_tag),
            &definition,
            None,
            vec![json!({"offset":0x690,"symbol":format!("{name}-solver")})],
        )?;
        render::build(
            c,
            &mut reader,
            &name,
            index,
            &entry,
            &model,
            mesh,
            &positions,
            &bones,
            model_tag,
            &model_template,
        )?;
        component::build(c, &mut reader, &name, &template, &source_owner)?;
        evidence.push(json!({"source_model":source_tag,"model":format!("{name}-model"),"definition":format!("{name}-definition"),"solver":format!("{name}-solver"),"simulation_converted":true,"float_vertices":vertices,"conversion":translated.report,"gameplay_verified":false}));
    }
    // The merged geometry remains the checked skinning source. Only its draws
    // move to the independently simulated owners, preserving vertex provenance.
    for stage in 0..23 {
        c.draws.records[stage].retain(|(_, material)| {
            c.draws
                .sources
                .get(material)
                .is_none_or(|model| !converted_models.contains(model))
        });
        c.draws.layout(stage)?;
    }
    c.graph.manifest["cloth"] = json!(evidence);
    Ok(())
}
