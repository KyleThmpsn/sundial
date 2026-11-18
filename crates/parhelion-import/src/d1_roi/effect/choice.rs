//! The native template graph whose owners a source effect graph's owners translate from.
use super::*;

/// Stock owners that join a fallback template from a lender graph, by family: the owner's tag,
/// its payload and the lender graph that registers it.
pub(super) type Borrowed = BTreeMap<Family, (u32, Payload, Payload)>;

pub(super) struct Choice {
    pub(super) template: Template,
    /// Empty unless the template is a fallback one.
    pub(super) borrowed: Borrowed,
}

/// The first listed template that fits the source families, or failing that the first fallback
/// template whose missing families can be borrowed. Templates are read in list order.
pub(super) fn choose(
    native: &mut Reader,
    owners: &[intake::Owner],
    families: &BTreeSet<Family>,
) -> Result<Choice> {
    if let Some(template) = listed(native, families)? {
        return Ok(Choice {
            template,
            borrowed: Borrowed::new(),
        });
    }
    if let Some((template, borrowed)) = composite(native, families)? {
        return Ok(Choice { template, borrowed });
    }
    match fallback(native, owners, families) {
        Ok((template, borrowed)) => Ok(Choice { template, borrowed }),
        Err(error) => {
            crate::cancellation::check()?;
            let template = without_context(native, families)?.ok_or(error)?;
            Ok(Choice {
                template,
                borrowed: Borrowed::new(),
            })
        }
    }
}

/// Failing all of those, a listed template may also add the shared effect context, which leaves
/// with its rows as an added channel bank or receiver does. Rise of Iron sword attack entity
/// `80A88C80` holds a network, a receiver, a sequence, the category and a channel bank. None of
/// the 46,235 stock graphs has exactly those owners, while 34 add only the effect context,
/// `80C199A2` among them.
fn without_context(native: &mut Reader, families: &BTreeSet<Family>) -> Result<Option<Template>> {
    for &candidate in &TEMPLATES {
        let template = Template::read(native, candidate)?;
        let provided = template.owners.keys().copied().collect::<BTreeSet<_>>();
        if provided.contains(&Family::Context)
            && provided.is_superset(families)
            && provided.difference(families).all(|family| {
                matches!(family, Family::Channel | Family::Receiver | Family::Context)
            })
        {
            return Ok(Some(template));
        }
    }
    Ok(None)
}

/// Families a graph may take from another stock graph when no stock graph holds its whole set.
/// A condition volume carries its own query, recipient and Timer, and its one channel getter is
/// found by name, so it needs nothing from the graph beside it.
const COMPOSED: [Family; 1] = [Family::Volume];

/// A source set no stock graph holds, as one listed template that holds the rest and a lender for
/// each [`COMPOSED`] family. Skorri's Harmony's `80A88C3E` has a volume, the framework network, a
/// status, a sequence and a channel bank. None of the 46,235 stock graphs has exactly those
/// owners, while `80FF06FD` holds the last four and `80FEEBEF` a volume of the same script. Each
/// translated owner keeps the stock graph that registers its own template.
fn composite(
    native: &mut Reader,
    families: &BTreeSet<Family>,
) -> Result<Option<(Template, Borrowed)>> {
    let missing = families
        .iter()
        .copied()
        .filter(|family| COMPOSED.contains(family))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(None);
    }
    let rest = families
        .iter()
        .copied()
        .filter(|family| !COMPOSED.contains(family))
        .collect::<BTreeSet<_>>();
    for &candidate in &TEMPLATES {
        let template = Template::read(native, candidate)?;
        if template.owners.keys().copied().collect::<BTreeSet<_>>() != rest {
            continue;
        }
        let mut borrowed = Borrowed::new();
        for &family in &missing {
            for &lender in &TEMPLATES {
                let lender = Template::read(native, lender)?;
                if let Some((_, tag, owner)) = lender.owners.get(&family) {
                    borrowed.insert(family, (*tag, owner.clone(), lender.graph.clone()));
                    break;
                }
            }
        }
        if borrowed.len() == missing.len() {
            return Ok(Some((template, borrowed)));
        }
    }
    Ok(None)
}

/// An exact family match wins. Otherwise a template may add only a channel bank and a receiver,
/// whose rows then have no source counterpart and are left out with them, as Rise of Iron machine
/// gun spawn `80A6A704`, with effect context, sequence and category owners only, takes
/// `80B80F16`. The first such superset is kept while the exact search continues.
fn listed(native: &mut Reader, families: &BTreeSet<Family>) -> Result<Option<Template>> {
    let mut superset = None;
    for &candidate in &TEMPLATES {
        let template = Template::read(native, candidate)?;
        let provided = template.owners.keys().copied().collect::<BTreeSet<_>>();
        if provided == *families {
            return Ok(Some(template));
        }
        if superset.is_none()
            && provided.is_superset(families)
            && provided
                .difference(families)
                .all(|family| matches!(family, Family::Channel | Family::Receiver))
        {
            superset = Some(template);
        }
    }
    Ok(superset)
}

/// Failing both, a fallback template may lack families whose source owner is a shared framework
/// owner. Their stock owners join from a lender graph that names the same stock owner, keeping
/// that graph's registrations and unbound requests, since each component's registrations come
/// from its own entity.
fn fallback(
    native: &mut Reader,
    owners: &[intake::Owner],
    families: &BTreeSet<Family>,
) -> Result<(Template, Borrowed)> {
    // The stock counterpart of each family whose source owner is a framework owner.
    let shared = owners
        .iter()
        .filter_map(|(_, owner, _, family)| {
            let family = (*family)?;
            FRAMEWORK
                .iter()
                .find(|(source, _)| source == owner)
                .map(|&(_, stock)| (family, stock))
        })
        .collect::<BTreeMap<_, _>>();
    let mut found = None;
    for &candidate in &FALLBACK_TEMPLATES {
        let template = Template::read(native, candidate)?;
        let provided = template.owners.keys().copied().collect::<BTreeSet<_>>();
        let missing = families.difference(&provided).copied().collect::<Vec<_>>();
        if !provided
            .difference(families)
            .all(|family| matches!(family, Family::Channel | Family::Receiver))
            || !missing.iter().all(|family| shared.contains_key(family))
        {
            continue;
        }
        let borrowed = borrow(native, &missing, &shared)?;
        if families
            .iter()
            .all(|family| template.owners.contains_key(family) || borrowed.contains_key(family))
        {
            found = Some((template, borrowed));
            break;
        }
    }
    found.context("No native effect template has the source graph's owner families")
}

/// Each missing family's stock owner from the first listed template or lender graph that names
/// the family's shared stock owner. A family no graph names is left out.
fn borrow(
    native: &mut Reader,
    missing: &[Family],
    shared: &BTreeMap<Family, u32>,
) -> Result<Borrowed> {
    let mut borrowed = Borrowed::new();
    for &family in missing {
        for &lender in TEMPLATES.iter().chain(&LENDERS) {
            let lender = Template::read(native, lender)?;
            if let Some((_, tag, owner)) = lender.owners.get(&family)
                && shared.get(&family) == Some(tag)
            {
                borrowed.insert(family, (*tag, owner.clone(), lender.graph.clone()));
                break;
            }
        }
    }
    Ok(borrowed)
}
