use super::*;

pub(crate) fn entry(text: &str) -> Result<&str> {
    let mut functions = text.split("define void @").skip(1);
    let function = functions.next().context("Missing DXIL entry point")?;
    ensure!(
        functions.next().is_none(),
        "Multiple DXIL executable functions"
    );
    let (name, body) = function
        .split_once("() {")
        .context("Unsupported DXIL entry point")?;
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "Invalid DXIL entry point name"
    );
    Ok(body
        .split_once("\n}")
        .context("Unterminated DXIL entry point")?
        .0)
}

pub(super) fn signature(text: &str, name: &str) -> Result<Vec<Field>> {
    let table = text.split_once(name).context("Missing DXIL signature")?.1;
    let mut fields = Vec::new();
    let mut started = false;
    for line in table.lines() {
        let words = line
            .trim_start_matches(';')
            .split_whitespace()
            .collect::<Vec<_>>();
        if words.len() >= 6 && words[1].parse::<u32>().is_ok() && words[3].parse::<u32>().is_ok() {
            ensure!(
                matches!(words[5], "float" | "uint"),
                "Unsupported DXIL signature format"
            );
            let mask = words[2];
            ensure!(
                ["x", "xy", "xyz", "xyzw"].contains(&mask),
                "Noncontiguous DXIL signature mask"
            );
            ensure!(
                words[0]
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                "Invalid DXIL semantic"
            );
            fields.push(Field {
                semantic: words[0].into(),
                index: words[1].parse()?,
                width: mask.len(),
                integer: words[5] == "uint",
            });
            started = true;
        } else if started {
            break;
        }
    }
    ensure!(fields.len() <= 32, "Excessive DXIL signature");
    Ok(fields)
}

impl Program {
    pub fn parse(text: &str) -> Result<Self> {
        let inputs = signature(text, "; Input signature:")?;
        let outputs = signature(text, "; Output signature:")?;
        ensure!(
            outputs
                .iter()
                .all(|field| field.semantic.eq_ignore_ascii_case("SV_Target")),
            "Unsupported DXIL output semantic"
        );
        let body = entry(text)?;
        let mut blocks = BTreeMap::from([(0, Vec::new())]);
        let mut order = vec![0];
        let mut current = 0;
        let mut identifiers = BTreeSet::new();
        for line in body.lines() {
            let line = line.trim();
            let label = line
                .strip_prefix("; <label>:")
                .and_then(|s| s.split_whitespace().next())
                .or_else(|| {
                    line.split_once(':')
                        .map(|(s, _)| s)
                        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                });
            if let Some(label) = label {
                current = label.parse()?;
                ensure!(
                    blocks.insert(current, Vec::new()).is_none(),
                    "Repeated DXIL block"
                );
                order.push(current);
                continue;
            }
            let code = line.split(';').next().unwrap_or("").trim();
            if code.is_empty() {
                continue;
            }
            let (ident, code) = if let Some((lhs, rhs)) = code.split_once(" = ") {
                (Some(id(lhs)?), rhs)
            } else {
                (None, code)
            };
            if let Some(ident) = ident {
                ensure!(identifiers.insert(ident), "Repeated DXIL SSA identifier");
            }
            blocks
                .get_mut(&current)
                .context("Missing DXIL block")?
                .push(Instruction {
                    id: ident,
                    code: code.into(),
                });
        }
        ensure!(
            blocks.len() <= 4096 && identifiers.len() <= 65536,
            "DXIL exceeds translation budget"
        );
        let mut edges = BTreeMap::new();
        for (&block, lines) in &blocks {
            let code = &lines.last().context("Empty DXIL block")?.code;
            let successors = if code == "ret void" {
                Vec::new()
            } else if let Some(branch) = code.strip_prefix("br ") {
                let args = arguments(branch)?;
                ensure!(
                    args.len() == 1 || args.len() == 3,
                    "Unsupported DXIL branch"
                );
                args.iter()
                    .skip(if args.len() == 3 { 1 } else { 0 })
                    .map(|arg| {
                        ensure!(arg.starts_with("label %"), "Invalid DXIL branch target");
                        id(arg)
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                bail!("Unsupported DXIL terminator {code}");
            };
            ensure!(
                successors.iter().all(|s| blocks.contains_key(s)),
                "Missing DXIL branch destination"
            );
            edges.insert(block, successors);
        }
        fn visit(
            block: u32,
            edges: &BTreeMap<u32, Vec<u32>>,
            active: &mut BTreeSet<u32>,
            post: &mut BTreeMap<u32, BTreeSet<u32>>,
        ) -> Result<BTreeSet<u32>> {
            if let Some(value) = post.get(&block) {
                return Ok(value.clone());
            }
            ensure!(
                active.len() < 512 && active.insert(block),
                "DXIL contains a loop or excessive nesting"
            );
            let successors = edges[&block]
                .iter()
                .map(|s| visit(*s, edges, active, post))
                .collect::<Result<Vec<_>>>()?;
            let mut result = successors.first().cloned().unwrap_or_default();
            for successor in successors.iter().skip(1) {
                result.retain(|v| successor.contains(v));
            }
            result.insert(block);
            active.remove(&block);
            post.insert(block, result.clone());
            Ok(result)
        }
        let mut post = BTreeMap::new();
        visit(0, &edges, &mut BTreeSet::new(), &mut post)?;
        ensure!(
            post.len() == blocks.len(),
            "DXIL contains unreachable blocks"
        );
        Ok(Self {
            inputs,
            outputs,
            blocks,
            order,
            edges,
            post,
        })
    }
}
