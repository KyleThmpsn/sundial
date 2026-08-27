use serde_json::Value;

pub(super) fn collect_change_summaries(before: &Value, after: &Value, limit: usize) -> Vec<String> {
    fn walk(before: &Value, after: &Value, path: &str, limit: usize, out: &mut Vec<String>) {
        if before == after || out.len() >= limit {
            return;
        }
        match (before, after) {
            (Value::Object(before), Value::Object(after)) => {
                let mut keys = before.keys().chain(after.keys()).collect::<Vec<_>>();
                keys.sort_unstable();
                keys.dedup();
                for key in keys {
                    if out.len() >= limit {
                        break;
                    }
                    let child_path = format!("{path}/{}", escape_json_pointer(key));
                    match (before.get(key), after.get(key)) {
                        (Some(before), Some(after)) => walk(before, after, &child_path, limit, out),
                        (Some(before), None) => out.push(format!(
                            "{child_path}: removed {}",
                            compact_change_value(before)
                        )),
                        (None, Some(after)) => out.push(format!(
                            "{child_path}: added {}",
                            compact_change_value(after)
                        )),
                        (None, None) => {}
                    }
                }
            }
            (Value::Array(before), Value::Array(after)) => {
                let shared = before.len().min(after.len());
                for index in 0..shared {
                    if out.len() >= limit {
                        break;
                    }
                    walk(
                        &before[index],
                        &after[index],
                        &format!("{path}/{index}"),
                        limit,
                        out,
                    );
                }
                for (index, value) in before.iter().enumerate().skip(shared) {
                    if out.len() >= limit {
                        break;
                    }
                    out.push(format!(
                        "{path}/{index}: removed {}",
                        compact_change_value(value)
                    ));
                }
                for (index, value) in after.iter().enumerate().skip(shared) {
                    if out.len() >= limit {
                        break;
                    }
                    out.push(format!(
                        "{path}/{index}: added {}",
                        compact_change_value(value)
                    ));
                }
            }
            _ => out.push(format!(
                "{}: {} -> {}",
                if path.is_empty() { "/" } else { path },
                compact_change_value(before),
                compact_change_value(after)
            )),
        }
    }

    let mut changes = Vec::new();
    walk(before, after, "", limit, &mut changes);
    changes
}

fn compact_change_value(value: &Value) -> String {
    const MAX_CHARS: usize = 72;
    let raw = match value {
        Value::Array(values) => format!("[{} items]", values.len()),
        Value::Object(values) => format!("{{{} fields}}", values.len()),
        _ => value.to_string(),
    };
    if raw.chars().count() <= MAX_CHARS {
        raw
    } else {
        format!("{}...", raw.chars().take(MAX_CHARS - 3).collect::<String>())
    }
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
