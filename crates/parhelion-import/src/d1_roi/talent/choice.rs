//! Client labels can enumerate one shared step's runtime variants.
use super::*;

pub(super) struct Choice {
    pub shared_index: usize,
    pub variant_index: Option<usize>,
    pub offset: usize,
    pub text: usize,
    pub variants: Vec<usize>,
}

pub(super) fn read(
    shared: &Payload,
    client: &Payload,
    node: usize,
    display: usize,
) -> Result<Vec<Choice>> {
    let steps = shared.array(node + 24, 144, Some(0x80801758))?;
    let labels = client.array(display, 48, Some(0x80803468))?;
    if steps.len() == labels.len() {
        return steps
            .into_iter()
            .zip(labels)
            .enumerate()
            .map(|(shared_index, (offset, text))| {
                Ok(Choice {
                    shared_index,
                    variant_index: None,
                    offset,
                    text,
                    variants: shared.array(offset, 88, Some(0x808016DE))?,
                })
            })
            .collect();
    }
    let [offset] = steps.as_slice() else {
        anyhow::bail!(
            "Source talent has {} shared steps and {} client labels",
            steps.len(),
            labels.len()
        );
    };
    let variants = shared.array(*offset, 88, Some(0x808016DE))?;
    ensure!(
        variants.len() == labels.len() && !labels.is_empty(),
        "Source talent variant and client label counts disagree"
    );
    Ok(variants
        .into_iter()
        .zip(labels)
        .enumerate()
        .map(|(variant_index, (variant, text))| Choice {
            shared_index: 0,
            variant_index: Some(variant_index),
            offset: *offset,
            text,
            variants: vec![variant],
        })
        .collect())
}
