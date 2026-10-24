//! Translate fields whose native consumers differ from the source layouts.
use super::*;
use schema::{Member, Schema};
use source::Decoded;

struct Lower<'a> {
    source: BTreeMap<usize, Decoded>,
    schema: &'a Schema,
    objects: BTreeMap<usize, Object>,
    limits: Vec<Value>,
}

fn take(fields: &mut serde_json::Map<String, Value>, name: &str) -> Result<Value> {
    fields
        .remove(name)
        .with_context(|| format!("Missing cloth field {name}"))
}

fn require(fields: &mut serde_json::Map<String, Value>, name: &str, expected: Value) -> Result<()> {
    ensure!(
        take(fields, name)? == expected,
        "Unsupported cloth field {name}"
    );
    Ok(())
}

pub(super) fn translate(
    source: BTreeMap<usize, Decoded>,
    schema: &Schema,
) -> Result<(BTreeMap<usize, Object>, Vec<Value>)> {
    let mut lower = Lower {
        source,
        schema,
        objects: BTreeMap::new(),
        limits: vec![json!({
        "contract":"Native simulation extension","detail":"Native velocity summaries and unstuck overrides remain disabled."})],
    };
    lower.object(1, 0)?;
    Ok((lower.objects, lower.limits))
}

impl Lower<'_> {
    fn expand(&mut self, value: Value, depth: usize) -> Result<Value> {
        ensure!(
            depth < 128,
            "Cloth graph expansion exceeds supported bounds"
        );
        match value {
            Value::Object(mut fields) if fields.len() == 1 && fields.contains_key("item") => {
                let index = integer(&take(&mut fields, "item")?)?;
                let item = self
                    .source
                    .get(&index)
                    .context("Missing source cloth reference")?
                    .clone();
                if item.flag == 16 {
                    self.object(index, depth + 1)?;
                    Ok(json!({"object":index}))
                } else {
                    self.expand(item.object.value, depth + 1)
                }
            }
            Value::Object(mut fields) if fields.len() == 1 && fields.contains_key("value") => {
                take(&mut fields, "value")
            }
            Value::Object(fields) => Ok(Value::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| Ok((k, self.expand(v, depth + 1)?)))
                    .collect::<Result<_>>()?,
            )),
            Value::Array(values) => Ok(json!(
                values
                    .into_iter()
                    .map(|v| self.expand(v, depth + 1))
                    .collect::<Result<Vec<_>>>()?
            )),
            other => Ok(other),
        }
    }

    fn ordered_ids(&self, value: &Value, field: &str) -> Result<()> {
        if value.is_null() {
            return Ok(());
        }
        let rows = &self
            .source
            .get(&integer(&value["item"])?)
            .context("Cloth index array")?
            .object
            .value;
        for (index, row) in array(rows)?.iter().enumerate() {
            let object = &self
                .source
                .get(&integer(&row["item"])?)
                .context("Cloth indexed object")?
                .object
                .value[0];
            let id = if object[field].is_object() {
                &object[field]["value"]
            } else {
                &object[field]
            };
            ensure!(
                integer(id)? == index,
                "Cloth {field} differs from native array ordering"
            );
        }
        Ok(())
    }

    fn dependency(&mut self, graph: Value, operators: &Value, depth: usize) -> Result<()> {
        let index = integer(&graph["item"])?;
        let raw = self
            .source
            .get(&index)
            .context("Missing cloth dependency graph")?
            .clone();
        ensure!(
            raw.object.name == "hclStateDependencyGraph",
            "Cloth dependency graph type differs"
        );
        let graph = self.expand(raw.object.value[0].clone(), depth + 1)?;
        let count = array(&self.expand(operators.clone(), depth + 1)?)?.len();
        let parents = array(&graph["parents"])?;
        let children = array(&graph["children"])?;
        ensure!(
            parents.len() == count && children.len() == count,
            "Cloth dependency extent differs"
        );
        for i in 0..count {
            for child in array(&children[i])? {
                let child = integer(child)?;
                ensure!(
                    i < child && child < count && array(&parents[child])?.contains(&json!(i)),
                    "Cloth operators are not in dependency order"
                );
            }
            for parent in array(&parents[i])? {
                let parent = integer(parent)?;
                ensure!(
                    parent < i && array(&children[parent])?.contains(&json!(i)),
                    "Cloth dependency edge is inconsistent"
                );
            }
        }
        let mut partition = Vec::new();
        for branch in array(&graph["branches"])? {
            partition.extend(
                array(&branch["stateOperatorIndices"])?
                    .iter()
                    .map(integer)
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        partition.sort_unstable();
        ensure!(
            partition == (0..count).collect::<Vec<_>>(),
            "Cloth dependency branches do not partition the operators"
        );
        Ok(())
    }

    fn simulation(&mut self, raw: &mut serde_json::Map<String, Value>, depth: usize) -> Result<()> {
        ensure!(
            zeros(&take(raw, "virtualCollisionPointsData")?),
            "Virtual cloth collision points require a separate translation"
        );
        let fixed = self.expand(take(raw, "fixedParticles")?, depth + 1)?;
        let particles = self.expand(raw["particleDatas"].clone(), depth + 1)?;
        let expected = array(&particles)?
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (p["invMass"].as_f64() == Some(0.)).then_some(json!(i)))
            .collect::<Vec<_>>();
        ensure!(
            array(&fixed)? == expected,
            "Fixed cloth particles disagree with inverse masses"
        );
        self.ordered_ids(&raw["staticConstraintSets"], "constraintId")?;
        take(raw, "simOpIds")?;
        let mut moved = Vec::new();
        for name in [
            "pinchDetectionEnabled",
            "landscapeCollisionEnabled",
            "transferMotionEnabled",
        ] {
            moved.push((name, take(raw, name)?));
        }
        ensure!(
            moved
                .iter()
                .filter(|(name, _)| *name != "landscapeCollisionEnabled")
                .all(|(_, v)| zeros(v)),
            "Cloth pinch detection and motion transfer require separate native validation"
        );
        let tolerance = raw["landscapeCollisionData"]
            .as_object_mut()
            .context("Cloth landscape settings")?
            .remove("collisionTolerance")
            .context("Cloth collision tolerance")?;
        let info = raw["simulationInfo"]
            .as_object_mut()
            .context("Cloth simulation settings")?;
        for (name, value) in moved {
            info.insert(name.into(), value);
        }
        info.insert("collisionTolerance".into(), tolerance);
        for name in [
            "computeVelocity",
            "overrideUnstuckParticles",
            "maxUnstuckParticlesLinkPerFrame",
        ] {
            info.insert(name.into(), json!(0));
        }
        Ok(())
    }

    fn object(&mut self, index: usize, depth: usize) -> Result<()> {
        ensure!(depth < 128, "Cloth object nesting exceeds supported bounds");
        if self.objects.contains_key(&index) {
            return Ok(());
        }
        let row = self
            .source
            .get(&index)
            .context("Source cloth object")?
            .clone();
        let name = row.object.name.clone();
        let mut raw = row.object.value[0]
            .as_object()
            .context("Cloth object fields")?
            .clone();
        require(&mut raw, "propertyBag", Value::Null)?;
        if raw.contains_key("operatorID") {
            for field in ["operatorID", "usedBuffers", "usedTransformSets"] {
                take(&mut raw, field)?;
            }
        }
        raw.remove("constraintId");
        match name.as_str() {
            "hclClothData" => {
                require(&mut raw, "stateTransitions", Value::Null)?;
                self.ordered_ids(&raw["operators"], "operatorID")?;
            }
            "hclClothState" => {
                let graph = take(&mut raw, "dependencyGraph")?;
                self.dependency(graph, &raw["operators"], depth)?;
            }
            "hclSimClothData" => self.simulation(&mut raw, depth)?,
            "hclSimulateOperator" => {
                let configs = self.expand(take(&mut raw, "simulateOpConfigs")?, depth + 1)?;
                let configs = array(&configs)?;
                ensure!(
                    configs.len() == 1,
                    "Multiple cloth simulation configurations are unsupported"
                );
                let config = &configs[0];
                ensure!(
                    config["useAllInstanceCollidables"] == 1
                        && config["adaptConstraintStiffness"] == 1
                        && array(&config["instanceCollidablesUsed"])?.is_empty(),
                    "Cloth simulation configuration differs from the native contract"
                );
                for field in ["subSteps", "numberOfSolveIterations", "constraintExecution"] {
                    raw.insert(field.into(), config[field].clone());
                }
                raw.insert("computeVelocityParticles".into(), json!([]));
            }
            "hclBendStiffnessConstraintSet" => {
                require(&mut raw, "clampBendStiffness", json!(1))?;
                let height = take(&mut raw, "maxRestPoseHeightSq")?;
                ensure!(
                    height.as_f64().is_some_and(|v| v >= 0.),
                    "Invalid cloth rest-pose height"
                );
                raw.insert("linksQuantizedWeights".into(), json!([]));
                self.limits.push(json!({"object":index,"contract":"Bend stiffness","source_max_rest_pose_height_sq":height,
                    "detail":"Native bending consumes the stored signed link coefficients. Modern clamp equivalence is not established."}));
            }
            "hclTransitionConstraintSet" => {
                let rows = self.expand(raw["perParticleData"].clone(), depth + 1)?;
                let mut vertices = Vec::new();
                let mut params = Vec::new();
                for row in array(&rows)? {
                    let mut row = row
                        .as_object()
                        .context("Cloth transition particle")?
                        .clone();
                    vertices.push(json!({"particleIndex":take(&mut row,"particleIndex")?,"referenceVertex":take(&mut row,"referenceVertex")?}));
                    params.push(Value::Object(row));
                }
                ensure!(!params.is_empty(), "Cloth transition has no particles");
                for param in &params {
                    for field in ["toAnimDelay", "toSimDelay", "toSimMaxDistance"] {
                        ensure!(
                            param[field]
                                .as_f64()
                                .is_some_and(|v| v.is_finite() && v >= 0.),
                            "Invalid cloth particle transition parameter {field}"
                        );
                    }
                }
                if params.iter().all(|p| p == &params[0]) {
                    raw.insert("perParticleData".into(), json!(vertices));
                    raw.insert("perParticleParams".into(), json!([params[0]]));
                } else {
                    // The native to-simulation consumer exits its entire loop when a
                    // particle completes. Descending delays keep unfinished particles
                    // before that exit, while retaining each particle/reference pair.
                    let mut paired: Vec<_> = vertices.into_iter().zip(params).collect();
                    paired.sort_by(|a, b| {
                        b.1["toSimDelay"]
                            .as_f64()
                            .unwrap()
                            .total_cmp(&a.1["toSimDelay"].as_f64().unwrap())
                    });
                    let (vertices, params): (Vec<_>, Vec<_>) = paired.into_iter().unzip();
                    raw.insert("perParticleData".into(), json!(vertices));
                    raw.insert("perParticleParams".into(), json!(params));
                }
            }
            "hclCollidable" => {
                for (name, value) in [
                    ("enabled", 1),
                    ("virtualCollisionPointCollisionEnabled", 0),
                    ("userData", 0),
                ] {
                    require(&mut raw, name, json!(value))?;
                }
                raw.insert("encouterredOOM".into(), json!(0));
            }
            "hclObjectSpaceMeshMeshDeformPOperator" => {
                require(&mut raw, "customSkinDeform", json!(0))?;
                let previous = row
                    .previous
                    .context("Missing cloth previous-buffer extension")?;
                ensure!(
                    previous.iter().all(|i| *i < u32::MAX),
                    "Invalid previous cloth buffer"
                );
                raw.insert("transferSimulation".into(), json!(1));
                raw.insert("inputBufferPrevIdx".into(), json!(previous[0]));
                raw.insert("outputBufferPrevIdx".into(), json!(previous[1]));
            }
            _ => {}
        }
        self.objects.insert(
            index,
            Object {
                name: name.clone(),
                value: Value::Null,
            },
        );
        let expanded = self.expand(Value::Object(raw), depth + 1)?;
        let value = self.record(&name, expanded)?;
        self.objects
            .get_mut(&index)
            .context("Native cloth object")?
            .value = value;
        Ok(())
    }

    fn record(&self, name: &str, value: Value) -> Result<Value> {
        let mut fields = value.as_object().context("Native cloth fields")?.clone();
        if name == "hclObjectSpaceDeformer" {
            for field in [
                "eightBlendEntries",
                "sevenBlendEntries",
                "sixBlendEntries",
                "fiveBlendEntries",
            ] {
                require(&mut fields, field, Value::Null)?;
            }
            fields.insert("batchSizeSpu".into(), json!(512));
        }
        if name == "hclTransformSetUsage" {
            take(&mut fields, "perComponentTransformTrackers")?;
        }
        let mut result = serde_json::Map::new();
        for member in self.schema.members(name)? {
            result.insert(
                member.name.clone(),
                self.field(&member, take(&mut fields, &member.name)?)?,
            );
        }
        ensure!(
            fields.is_empty(),
            "Untranslated {name} fields: {:?}",
            fields.keys()
        );
        Ok(Value::Object(result))
    }

    fn field(&self, member: &Member, value: Value) -> Result<Value> {
        if member.length != 0 {
            let mut values = array(&value)?.to_vec();
            if !values.is_empty()
                && values.iter().all(|v| {
                    v.as_object()
                        .is_some_and(|m| m.len() == 1 && m.contains_key("values"))
                })
            {
                values = values
                    .iter()
                    .map(|v| array(&v["values"]))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect();
            }
            ensure!(
                values.len() == member.length,
                "Native cloth fixed array differs"
            );
            let mut element = member.clone();
            element.length = 0;
            return Ok(json!(
                values
                    .into_iter()
                    .map(|v| self.field(&element, v))
                    .collect::<Result<Vec<_>>>()?
            ));
        }
        if member.kind == 22 {
            let mut element = member.clone();
            element.kind = member.subtype;
            return Ok(json!(
                array(&value)?
                    .iter()
                    .map(|v| self.field(&element, v.clone()))
                    .collect::<Result<Vec<_>>>()?
            ));
        }
        if member.kind == 25 {
            return self.record(
                member.class.as_deref().context("Native cloth struct")?,
                value,
            );
        }
        Ok(value)
    }
}

pub(super) fn remap(objects: &mut BTreeMap<usize, Object>, mapping: &[u16]) -> Result<()> {
    ensure!(
        !mapping.is_empty() && mapping.iter().all(|i| *i < 256 || *i == u16::MAX),
        "Invalid native cloth bone map"
    );
    let count = usize::from(
        *mapping
            .iter()
            .filter(|i| **i != u16::MAX)
            .max()
            .context("Native cloth palette")?,
    ) + 1;
    for object in objects.values_mut() {
        let target = match object.name.as_str() {
            "hclTransformSetDefinition" => {
                ensure!(
                    integer(&object.value["numTransforms"])? <= mapping.len(),
                    "Cloth bone map does not cover its palette"
                );
                object.value["numTransforms"] = json!(count);
                None
            }
            "hclObjectSpaceSkinPNTOperator" => Some(&mut object.value["transformSubset"]),
            "hclSimClothData" => {
                Some(&mut object.value["collidableTransformMap"]["transformIndices"])
            }
            _ => None,
        };
        if let Some(values) = target {
            for index in values.as_array_mut().context("Cloth transform indexes")? {
                let mapped = *mapping
                    .get(integer(index)?)
                    .context("Cloth transform is outside the bone map")?;
                ensure!(
                    mapped != u16::MAX,
                    "Cloth transform has no compatible native bone"
                );
                *index = json!(mapped);
            }
        }
    }
    Ok(())
}
