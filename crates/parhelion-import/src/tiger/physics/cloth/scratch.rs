//! Fold scratch buffers into the display buffers that already hold their values.
//!
//! Shadowkeep's cloth step gives a scratch buffer storage only when its world requests step temp
//! memory, which the game's cloth path never does. An operator that writes one writes through a
//! null or stale pointer. Source universal ornaments fill each scratch buffer with a
//! position/normal skin that repeats a position/normal/tangent skin into a display buffer, with the
//! same transforms, deformer and packed positions and normals. Wherever the scratch buffer is read,
//! that display buffer holds the same positions and normals, so its readers take the display buffer
//! and the repeated skin leaves the cloth. The scratch definitions stay in place unused, so buffer
//! indexes and render bindings keep their meaning.
use super::*;

/// Buffer fields whose positions or normals an operator reads.
fn inputs(class: &str) -> &'static [&'static str] {
    match class {
        "hclMoveParticlesOperator" => &["refBufferIdx"],
        "hclGatherAllVerticesOperator" | "hclCopyVerticesOperator" => &["inputBufferIdx"],
        "hclObjectSpaceMeshMeshDeformPOperator" => &["inputBufferIdx", "inputBufferPrevIdx"],
        _ => &[],
    }
}

/// Buffer fields whose positions or normals an operator writes. Vertex frame updates write
/// tangents only.
fn outputs(class: &str) -> &'static [&'static str] {
    match class {
        "hclObjectSpaceSkinPNTOperator" | "hclObjectSpaceSkinPNOperator" => &["outputBufferIndex"],
        "hclGatherAllVerticesOperator" | "hclCopyVerticesOperator" => &["outputBufferIdx"],
        "hclObjectSpaceMeshMeshDeformPOperator" => &["outputBufferIdx", "outputBufferPrevIdx"],
        _ => &[],
    }
}

fn names(op: &Object, fields: &[&str], buffer: usize) -> bool {
    fields
        .iter()
        .any(|field| op.value[*field].as_u64() == Some(buffer as u64))
}

fn ids(value: &Value) -> Result<Vec<usize>> {
    array(value)?
        .iter()
        .map(|r| integer(&r["object"]))
        .collect()
}

fn get(objects: &BTreeMap<usize, Object>, id: usize) -> Result<&Object> {
    objects.get(&id).context("Cloth object reference is absent")
}

/// Whether a position/normal skin writes exactly the positions and normals that a
/// position/normal/tangent skin writes.
fn repeats(pn: &Value, pnt: &Value) -> Result<bool> {
    let ours = array(&pn["localPNs"])?;
    let theirs = array(&pnt["localPNTs"])?;
    Ok([
        "transformSetIndex",
        "transformSubset",
        "boneFromSkinMeshTransforms",
        "objectSpaceDeformer",
    ]
    .iter()
    .all(|field| pn[*field] == pnt[*field])
        && array(&pn["localUnpackedPNs"])?.is_empty()
        && array(&pnt["localUnpackedPNTs"])?.is_empty()
        && ours.len() == theirs.len()
        && ours.iter().zip(theirs).all(|(a, b)| {
            a["localPosition"] == b["localPosition"] && a["localNormal"] == b["localNormal"]
        }))
}

/// A scratch buffer, the display buffer that holds its values, the position/normal skin that
/// writes it and the position/normal/tangent skin it repeats, as operator positions.
struct Fold {
    scratch: usize,
    display: usize,
    writer: usize,
    partner: usize,
}

struct Cloth<'a> {
    objects: &'a BTreeMap<usize, Object>,
    operators: Vec<usize>,
    simulations: Vec<usize>,
}

impl Cloth<'_> {
    fn op(&self, position: usize) -> Result<&Object> {
        get(
            self.objects,
            *self
                .operators
                .get(position)
                .context("Cloth state names an absent operator")?,
        )
    }

    /// The transition constraints a simulation reads its reference mesh through.
    fn transitions(&self, op: &Object) -> Result<Vec<usize>> {
        if op.name != "hclSimulateOperator" {
            return Ok(Vec::new());
        }
        let simulation = *self
            .simulations
            .get(integer(&op.value["simClothIndex"])?)
            .context("Cloth simulation index is outside its definitions")?;
        let mut result = Vec::new();
        for id in ids(&get(self.objects, simulation)?.value["staticConstraintSets"])? {
            if get(self.objects, id)?.name == "hclTransitionConstraintSet" {
                result.push(id);
            }
        }
        Ok(result)
    }

    fn reads(&self, op: &Object, buffer: usize) -> Result<bool> {
        if names(op, inputs(&op.name), buffer) {
            return Ok(true);
        }
        Ok(self.transitions(op)?.iter().any(|id| {
            self.objects[id].value["referenceMeshBufferIdx"].as_u64() == Some(buffer as u64)
        }))
    }

    fn plan(&self, definitions: &[&Object], scratch: usize) -> Result<Fold> {
        let mut writers = Vec::new();
        let mut partners = Vec::new();
        for position in 0..self.operators.len() {
            let op = self.op(position)?;
            if names(op, outputs(&op.name), scratch) {
                writers.push(position);
            }
        }
        ensure!(
            writers.len() == 1 && self.op(writers[0])?.name == "hclObjectSpaceSkinPNOperator",
            "Cloth scratch buffer {scratch} needs a single position/normal skin"
        );
        let writer = &self.op(writers[0])?.value;
        for position in 0..self.operators.len() {
            let op = self.op(position)?;
            if op.name == "hclObjectSpaceSkinPNTOperator" && repeats(writer, &op.value)? {
                partners.push(position);
            }
        }
        ensure!(
            partners.len() == 1,
            "Cloth scratch buffer {scratch} does not repeat a single display skin"
        );
        let display = integer(&self.op(partners[0])?.value["outputBufferIndex"])?;
        let target = definitions
            .get(display)
            .context("Cloth buffer index is outside its definitions")?;
        ensure!(
            target.name == "hclBufferDefinition"
                && target.value["type"] == 4
                && target.value["numVertices"] == definitions[scratch].value["numVertices"],
            "Cloth scratch buffer {scratch} does not match its display buffer"
        );
        Ok(Fold {
            scratch,
            display,
            writer: writers[0],
            partner: partners[0],
        })
    }

    /// Each read of the scratch buffer in a state follows its skin, and the display buffer's most
    /// recent writer at that point is the skin it repeats.
    fn check(&self, order: &[usize], fold: &Fold) -> Result<()> {
        for (at, &position) in order.iter().enumerate() {
            if !self.reads(self.op(position)?, fold.scratch)? {
                continue;
            }
            let before = &order[..at];
            let mut last = None;
            for &earlier in before {
                let op = self.op(earlier)?;
                if names(op, outputs(&op.name), fold.display) {
                    last = Some(earlier);
                }
            }
            ensure!(
                before.contains(&fold.writer) && last == Some(fold.partner),
                "Cloth state reads scratch buffer {} without its repeated display skin",
                fold.scratch
            );
        }
        Ok(())
    }
}

/// Merges a scratch buffer's state access into its display buffer's access.
fn merge(used: &mut Vec<Value>, fold: &Fold) -> Result<()> {
    let index = |row: &Value, field: &str| row[field].as_u64().map(|i| i as usize);
    ensure!(
        used.iter()
            .all(|row| index(row, "shadowBufferIndex") != Some(fold.scratch)
                || index(row, "bufferIndex") == Some(fold.scratch)),
        "Cloth scratch buffer {} shadows another buffer",
        fold.scratch
    );
    let Some(at) = used
        .iter()
        .position(|row| index(row, "bufferIndex") == Some(fold.scratch))
    else {
        return Ok(());
    };
    let row = used.remove(at);
    let usage = &row["bufferUsage"];
    if zeros(usage) {
        return Ok(());
    }
    let target = used
        .iter_mut()
        .find(|row| index(row, "bufferIndex") == Some(fold.display))
        .context("Cloth state reads a scratch buffer without its display buffer")?;
    let flags = array(&usage["perComponentFlags"])?
        .iter()
        .map(integer)
        .collect::<Result<Vec<_>>>()?;
    let merged = target["bufferUsage"]["perComponentFlags"]
        .as_array_mut()
        .filter(|values| values.len() == flags.len())
        .context("Cloth buffer usage flags differ")?;
    for (value, flag) in merged.iter_mut().zip(flags) {
        *value = json!(integer(value)? | flag);
    }
    let read =
        integer(&usage["trianglesRead"])? | integer(&target["bufferUsage"]["trianglesRead"])?;
    target["bufferUsage"]["trianglesRead"] = json!(read);
    Ok(())
}

pub(super) fn fold(objects: &mut BTreeMap<usize, Object>, limits: &mut Vec<Value>) -> Result<()> {
    let containers = objects
        .values()
        .filter(|o| o.name == "hclClothContainer")
        .collect::<Vec<_>>();
    ensure!(containers.len() == 1, "Cloth container topology differs");
    let clothes = ids(&containers[0].value["clothDatas"])?;
    ensure!(
        clothes.len() == 1,
        "Cloth component requires one cloth definition"
    );
    let root = &get(objects, clothes[0])?.value;
    let definitions = ids(&root["bufferDefinitions"])?
        .into_iter()
        .map(|id| get(objects, id))
        .collect::<Result<Vec<_>>>()?;
    let scratch = definitions
        .iter()
        .enumerate()
        .filter(|(_, d)| d.name == "hclScratchBufferDefinition")
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if scratch.is_empty() {
        return Ok(());
    }
    let cloth = Cloth {
        objects,
        operators: ids(&root["operators"])?,
        simulations: ids(&root["simClothDatas"])?,
    };
    let folds = scratch
        .iter()
        .map(|&s| cloth.plan(&definitions, s))
        .collect::<Result<Vec<_>>>()?;
    let states = ids(&root["clothStateDatas"])?;
    for &state in &states {
        let order = array(&get(objects, state)?.value["operators"])?
            .iter()
            .map(integer)
            .collect::<Result<Vec<_>>>()?;
        for fold in &folds {
            cloth.check(&order, fold)?;
        }
    }
    // Readers and transition references move to the display buffers.
    let mut retargets = Vec::new();
    for position in 0..cloth.operators.len() {
        let op = cloth.op(position)?;
        for fold in &folds {
            for field in inputs(&op.name) {
                if op.value[*field].as_u64() == Some(fold.scratch as u64) {
                    retargets.push((cloth.operators[position], *field, fold.display));
                }
            }
            for id in cloth.transitions(op)? {
                if objects[&id].value["referenceMeshBufferIdx"].as_u64()
                    == Some(fold.scratch as u64)
                {
                    retargets.push((id, "referenceMeshBufferIdx", fold.display));
                }
            }
        }
    }
    let removed = folds.iter().map(|f| f.writer).collect::<BTreeSet<_>>();
    let writers = removed
        .iter()
        .map(|&p| cloth.operators[p])
        .collect::<Vec<_>>();
    let report = folds
        .iter()
        .map(|f| {
            json!({"scratch":f.scratch,"display":f.display,
                "removed_operator":cloth.objects[&cloth.operators[f.writer]].value["name"],
                "repeated_operator":cloth.objects[&cloth.operators[f.partner]].value["name"]})
        })
        .collect::<Vec<_>>();
    let root_id = clothes[0];
    for (id, field, display) in retargets {
        objects.get_mut(&id).context("Cloth reader")?.value[field] = json!(display);
    }
    // Operator positions after the removed skins move down.
    let renumber = |position: usize| position - removed.range(..position).count();
    for state in states {
        let value = &mut objects.get_mut(&state).context("Cloth state")?.value;
        let order = array(&value["operators"])?
            .iter()
            .map(integer)
            .collect::<Result<Vec<_>>>()?;
        value["operators"] = json!(
            order
                .into_iter()
                .filter(|p| !removed.contains(p))
                .map(renumber)
                .collect::<Vec<_>>()
        );
        let mut used = array(&value["usedBuffers"])?.to_vec();
        for fold in &folds {
            merge(&mut used, fold)?;
        }
        value["usedBuffers"] = json!(used);
    }
    let operators = &mut objects.get_mut(&root_id).context("Cloth definition")?.value["operators"];
    let kept = array(operators)?
        .iter()
        .enumerate()
        .filter(|(p, _)| !removed.contains(p))
        .map(|(_, r)| r.clone())
        .collect::<Vec<_>>();
    *operators = json!(kept);
    for id in writers {
        objects.remove(&id);
    }
    limits.push(json!({"contract":"Scratch buffers","folded":report,
        "detail":"Shadowkeep's cloth step gives scratch buffers no storage. Their readers take the display buffer that the repeated skin writes."}));
    Ok(())
}
