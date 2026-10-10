//! Cross-object bounds consumed by the native solver and its renderer bindings.
use super::*;

struct Graph<'a> {
    objects: &'a BTreeMap<usize, Object>,
    buffers: Vec<&'a Value>,
    transforms: Vec<&'a Value>,
    simulations: Vec<&'a Value>,
}

fn object<'a>(objects: &'a BTreeMap<usize, Object>, reference: &Value) -> Result<&'a Object> {
    objects
        .get(&integer(&reference["object"])?)
        .context("Cloth object reference is absent")
}
fn references<'a>(
    objects: &'a BTreeMap<usize, Object>,
    value: &Value,
    kind: &str,
) -> Result<Vec<&'a Value>> {
    array(value)?
        .iter()
        .map(|r| {
            let o = object(objects, r)?;
            ensure!(
                o.name == kind,
                "Cloth object has an unexpected type, expected {kind}"
            );
            Ok(&o.value)
        })
        .collect()
}
fn indices(value: &Value, count: usize) -> Result<()> {
    for i in array(value)? {
        ensure!(integer(i)? < count, "Cloth index exceeds its target");
    }
    Ok(())
}

impl Graph<'_> {
    fn buffer(&self, index: &Value) -> Result<&Value> {
        self.buffers
            .get(integer(index)?)
            .copied()
            .context("Cloth buffer index is outside its definitions")
    }
    fn vertices(&self, index: &Value) -> Result<usize> {
        integer(&self.buffer(index)?["numVertices"])
    }
    fn transform_count(&self, index: &Value) -> Result<usize> {
        integer(
            &self
                .transforms
                .get(integer(index)?)
                .context("Cloth transform set is absent")?["numTransforms"],
        )
    }
    fn simulation(&self, index: &Value) -> Result<&Value> {
        self.simulations
            .get(integer(index)?)
            .copied()
            .context("Cloth simulation index is outside its definitions")
    }
    fn deformer(&self, value: &Value, count: usize, inputs: usize, local: &Value) -> Result<()> {
        let d = &value["objectSpaceDeformer"];
        ensure!(
            integer(&d["startVertexIndex"])? <= integer(&d["endVertexIndex"])?
                && integer(&d["endVertexIndex"])? < count,
            "Cloth deformer vertex range exceeds its output"
        );
        let controls = array(&d["controlBytes"])?;
        ensure!(
            controls.len() == array(local)?.len(),
            "Cloth deformer blocks disagree with local data"
        );
        for (kind, field) in [
            "fourBlendEntries",
            "threeBlendEntries",
            "twoBlendEntries",
            "oneBlendEntries",
        ]
        .into_iter()
        .enumerate()
        {
            let entries = array(&d[field])?;
            ensure!(
                entries.len()
                    == controls
                        .iter()
                        .filter(|c| c.as_u64() == Some(kind as u64))
                        .count(),
                "Cloth blend controls disagree with block counts"
            );
            for entry in entries {
                indices(&entry["vertexIndices"], count)?;
                indices(&entry["boneIndices"], inputs)?;
            }
        }
        indices(&d["controlBytes"], 4)
    }
    fn operator(&self, op: &Object) -> Result<()> {
        let v = &op.value;
        match op.name.as_str() {
            "hclObjectSpaceSkinPNTOperator" => {
                let count = self.vertices(&v["outputBufferIndex"])?;
                let subset = array(&v["transformSubset"])?;
                indices(
                    &v["transformSubset"],
                    self.transform_count(&v["transformSetIndex"])?,
                )?;
                ensure!(
                    !subset.is_empty()
                        && subset.len() == array(&v["boneFromSkinMeshTransforms"])?.len()
                        && array(&v["localUnpackedPNTs"])?.is_empty(),
                    "Cloth skin transform or local data differs"
                );
                self.deformer(v, count, subset.len(), &v["localPNTs"])?;
            }
            "hclMoveParticlesOperator" => {
                let particles =
                    array(&self.simulation(&v["simClothIndex"])?["particleDatas"])?.len();
                let vertices = self.vertices(&v["refBufferIdx"])?;
                for pair in array(&v["vertexParticlePairs"])? {
                    ensure!(
                        integer(&pair["particleIndex"])? < particles
                            && integer(&pair["vertexIndex"])? < vertices,
                        "Cloth anchor exceeds its particle or reference vertex"
                    );
                }
            }
            "hclSimulateOperator" => {
                let sim = self.simulation(&v["simClothIndex"])?;
                let constraints = array(&sim["staticConstraintSets"])?.len();
                ensure!(
                    (1..=64).contains(&integer(&v["subSteps"])?)
                        && (1..=64).contains(&integer(&v["numberOfSolveIterations"])?),
                    "Cloth simulation iteration count exceeds supported bounds"
                );
                for index in array(&v["constraintExecution"])? {
                    ensure!(
                        index.as_i64() == Some(-1) || integer(index)? < constraints,
                        "Cloth solve order exceeds its constraints"
                    );
                }
            }
            "hclCopyVerticesOperator" => {
                let count = integer(&v["numberOfVertices"])?;
                ensure!(
                    integer(&v["startVertexIn"])?
                        .checked_add(count)
                        .is_some_and(|n| self.vertices(&v["inputBufferIdx"]).is_ok_and(|c| n <= c))
                        && integer(&v["startVertexOut"])?
                            .checked_add(count)
                            .is_some_and(|n| self
                                .vertices(&v["outputBufferIdx"])
                                .is_ok_and(|c| n <= c)),
                    "Cloth copy exceeds its vertex buffers"
                );
            }
            "hclGatherAllVerticesOperator" => {
                let input = self.vertices(&v["inputBufferIdx"])?;
                ensure!(
                    array(&v["vertexInputFromVertexOutput"])?.len()
                        == self.vertices(&v["outputBufferIdx"])?,
                    "Cloth gather differs from output length"
                );
                for index in array(&v["vertexInputFromVertexOutput"])? {
                    ensure!(
                        (v["partialGather"] == 1 && index.as_i64() == Some(-1))
                            || integer(index)? < input,
                        "Cloth gather exceeds its input"
                    );
                }
            }
            "hclUpdateSomeVertexFramesOperator" => {
                let count = self.vertices(&v["bufferIdx"])?;
                // This source profile reconstructs tangents from selected vertices.
                ensure!(
                    v["updateNormals"] == 0
                        && v["updateTangents"] == 1
                        && v["updateBiTangents"] == 0
                        && v["numUniqueNormalIDs"] == 0,
                    "Cloth frame reconstruction requires a separate native contract"
                );
                for field in [
                    "involvedTriangles",
                    "involvedVertexToNormalID",
                    "triangleFlips",
                    "biTangentFlip",
                ] {
                    ensure!(
                        array(&v[field])?.is_empty(),
                        "Unsupported cloth frame array {field}"
                    );
                }
                indices(&v["involvedVertices"], count)?;
                indices(&v["referenceVertices"], count)?;
                indices(
                    &v["selectionVertexToInvolvedVertex"],
                    array(&v["involvedVertices"])?.len(),
                )?;
                let selected = array(&v["selectionVertexToInvolvedVertex"])?.len();
                for field in [
                    "referenceVertices",
                    "tangentEdgeCosAngle",
                    "tangentEdgeSinAngle",
                ] {
                    ensure!(
                        array(&v[field])?.len() == selected,
                        "Cloth tangent arrays disagree"
                    );
                }
            }
            "hclObjectSpaceMeshMeshDeformPOperator" => {
                let input = self.buffer(&v["inputBufferIdx"])?;
                let output = self.vertices(&v["outputBufferIdx"])?;
                let triangles = array(&v["inputTrianglesSubset"])?;
                indices(&v["inputTrianglesSubset"], integer(&input["numTriangles"])?)?;
                let transforms = if triangles.is_empty() {
                    integer(&input["numTriangles"])?
                } else {
                    triangles.len()
                };
                ensure!(
                    transforms == array(&v["triangleFromMeshTransforms"])?.len()
                        && array(&v["localUnpackedPs"])?.is_empty(),
                    "Cloth transfer transforms differ"
                );
                ensure!(
                    self.vertices(&v["inputBufferPrevIdx"])? == integer(&input["numVertices"])?
                        && self.vertices(&v["outputBufferPrevIdx"])? == output,
                    "Cloth previous-position buffers differ"
                );
                self.deformer(v, output, transforms, &v["localPs"])?;
            }
            _ => anyhow::bail!("Unsupported native cloth operator {}", op.name),
        }
        Ok(())
    }
    fn validate_simulation(&self, sim: &Value) -> Result<Option<usize>> {
        let particles = array(&sim["particleDatas"])?;
        ensure!(
            !particles.is_empty() && particles.len() <= 65535,
            "Invalid cloth particle count"
        );
        for p in particles {
            for field in ["mass", "invMass", "radius", "friction"] {
                ensure!(
                    p[field].as_f64().is_some_and(|v| v >= 0.),
                    "Negative cloth particle property"
                );
            }
        }
        let triangles = array(&sim["triangleIndices"])?;
        ensure!(
            triangles.len().is_multiple_of(3)
                && array(&sim["triangleFlips"])?.len() == (triangles.len() / 3).div_ceil(8),
            "Cloth triangle flags differ"
        );
        indices(&sim["triangleIndices"], particles.len())?;
        for field in [
            "staticCollisionMasks",
            "perParticlePinchDetectionEnabledFlags",
        ] {
            ensure!(
                array(&sim[field])?.len() == particles.len(),
                "Cloth particle metadata length differs"
            );
        }
        let map = &sim["collidableTransformMap"];
        indices(
            &map["transformIndices"],
            self.transform_count(&map["transformSetIndex"])?,
        )?;
        let collidables = array(&sim["perInstanceCollidables"])?.len();
        ensure!(
            array(&map["transformIndices"])?.len() == collidables
                && array(&map["offsets"])?.len() == collidables
                && array(&sim["collidablePinchingDatas"])?.len() == collidables,
            "Cloth collider bindings differ"
        );
        ensure!(
            array(&sim["antiPinchConstraintSets"])?.is_empty()
                && array(&sim["actions"])?.is_empty(),
            "Unsupported cloth action or anti-pinch constraint"
        );
        for pose in references(self.objects, &sim["simClothPoses"], "hclSimClothPose")? {
            ensure!(
                array(&pose["positions"])?.len() == particles.len(),
                "Cloth pose length differs"
            );
        }
        let mut transition = None;
        for (index, reference) in array(&sim["staticConstraintSets"])?.iter().enumerate() {
            let constraint = object(self.objects, reference)?;
            let v = &constraint.value;
            match constraint.name.as_str() {
                "hclStandardLinkConstraintSet"
                | "hclStretchLinkConstraintSet"
                | "hclBendStiffnessConstraintSet" => {
                    for link in array(&v["links"])? {
                        for field in ["particleA", "particleB", "particleC", "particleD"] {
                            if let Some(value) = link.get(field) {
                                ensure!(
                                    integer(value)? < particles.len(),
                                    "Cloth link exceeds its particles"
                                );
                            }
                        }
                    }
                }
                "hclTransitionConstraintSet" => {
                    ensure!(
                        transition.replace(index).is_none(),
                        "Multiple cloth transition constraints are ambiguous"
                    );
                    let count = self.vertices(&v["referenceMeshBufferIdx"])?;
                    let mut seen = BTreeSet::new();
                    for row in array(&v["perParticleData"])? {
                        ensure!(
                            integer(&row["particleIndex"])? < particles.len()
                                && integer(&row["referenceVertex"])? < count,
                            "Cloth transition exceeds its particle or reference vertex"
                        );
                        ensure!(
                            seen.insert(integer(&row["particleIndex"])?),
                            "Cloth transition repeats a particle"
                        );
                    }
                    let params = array(&v["perParticleParams"])?;
                    ensure!(
                        params.len() == 1 || params.len() == seen.len(),
                        "Cloth transition parameter count differs from its particles"
                    );
                    for field in [
                        "toAnimPeriod",
                        "toAnimPlusDelayPeriod",
                        "toSimPeriod",
                        "toSimPlusDelayPeriod",
                    ] {
                        ensure!(
                            v[field].as_f64().is_some_and(|n| n > 0.),
                            "Invalid cloth transition duration"
                        );
                    }
                    for (delay, period, total) in [
                        ("toAnimDelay", "toAnimPeriod", "toAnimPlusDelayPeriod"),
                        ("toSimDelay", "toSimPeriod", "toSimPlusDelayPeriod"),
                    ] {
                        let maximum = params
                            .iter()
                            .map(|p| p[delay].as_f64().unwrap())
                            .fold(0., f64::max);
                        ensure!(
                            v[total].as_f64().unwrap() + 0.000_001
                                >= v[period].as_f64().unwrap() + maximum,
                            "Cloth transition duration does not cover its particle delays"
                        );
                    }
                }
                _ => anyhow::bail!("Unsupported cloth constraint {}", constraint.name),
            }
        }
        Ok(transition)
    }
}

pub(super) fn graph(objects: &BTreeMap<usize, Object>) -> Result<Value> {
    let containers = objects
        .values()
        .filter(|o| o.name == "hclClothContainer")
        .collect::<Vec<_>>();
    ensure!(
        containers.len() == 1 && array(&containers[0].value["collidables"])?.is_empty(),
        "Cloth container topology differs"
    );
    let cloths = references(objects, &containers[0].value["clothDatas"], "hclClothData")?;
    ensure!(
        cloths.len() == 1,
        "Cloth component requires one cloth definition"
    );
    let cloth = cloths[0];
    ensure!(
        array(&cloth["actions"])?.is_empty() && cloth["targetPlatform"] == 2,
        "Cloth action or platform differs"
    );
    let graph = Graph {
        objects,
        buffers: references(objects, &cloth["bufferDefinitions"], "hclBufferDefinition")?,
        transforms: references(
            objects,
            &cloth["transformSetDefinitions"],
            "hclTransformSetDefinition",
        )?,
        simulations: references(objects, &cloth["simClothDatas"], "hclSimClothData")?,
    };
    for t in &graph.transforms {
        ensure!(
            t["type"] == 1 && (1..=256).contains(&integer(&t["numTransforms"])?),
            "Unsupported cloth transform set"
        );
    }
    for buffer in &graph.buffers {
        let vertices = integer(&buffer["numVertices"])?;
        ensure!(
            (1..=65535).contains(&vertices),
            "Invalid cloth buffer length"
        );
        match integer(&buffer["type"])? {
            1 | 2 => {
                let sim = graph.simulation(&buffer["subType"])?;
                ensure!(
                    array(&sim["particleDatas"])?.len() == vertices
                        && array(&sim["triangleIndices"])?.len() / 3
                            == integer(&buffer["numTriangles"])?,
                    "Cloth simulation buffer topology differs"
                );
            }
            4 => ensure!(
                buffer["subType"] == 0,
                "Unsupported cloth display buffer subtype"
            ),
            _ => anyhow::bail!("Unsupported cloth buffer type"),
        }
    }
    let transitions = graph
        .simulations
        .iter()
        .map(|s| graph.validate_simulation(s))
        .collect::<Result<Vec<_>>>()?;
    let operators = array(&cloth["operators"])?;
    for operator in operators {
        graph.operator(object(objects, operator)?)?;
    }
    let mut states = Vec::new();
    for state in references(objects, &cloth["clothStateDatas"], "hclClothState")? {
        indices(&state["operators"], operators.len())?;
        indices(&state["usedSimCloths"], graph.simulations.len())?;
        for b in array(&state["usedBuffers"])? {
            graph.buffer(&b["bufferIndex"])?;
            graph.buffer(&b["shadowBufferIndex"])?;
        }
        for t in array(&state["usedTransformSets"])? {
            graph.transform_count(&t["transformSetIndex"])?;
        }
        let constraints = array(&state["usedSimCloths"])?
            .iter()
            .map(|i| Ok(transitions[integer(i)?]))
            .collect::<Result<BTreeSet<_>>>()?;
        states.push(json!({"name":state["name"],"transition_constraints":constraints}));
    }
    Ok(
        json!({"states":states,"buffers":graph.buffers.iter().map(|b| json!({"vertices":b["numVertices"],"triangles":b["numTriangles"],"type":b["type"],"layout":b["bufferLayout"]})).collect::<Vec<_>>()}),
    )
}
