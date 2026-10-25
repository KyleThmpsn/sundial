//! How Sunrise groups subclass items, and the order that keeps each group to one class.
//!
//! Sunrise takes every item whose socket-entry list has a Super lane, in item index order, and
//! groups them in threes from the first. A character that equips one is given the other two of
//! its group, and a last group of one or two groups nothing. The stock nine make three groups of
//! their own, and an authored item's index follows its place in the build, so the build places
//! each class's subclasses in threes and the ones left over last. The install gives each
//! subclass to its own class's characters either way.

/// The subclasses one group holds.
const GROUP: usize = 3;

/// The order to give the project's subclasses, as places in `classes`, which names each one's
/// class: each class's subclasses in threes, classes in the order they first appear and
/// subclasses in their own, then the ones left over. Two left over stand alone, and more share
/// groups across classes, which Sunrise leaves no other way to hold.
pub(crate) fn order(classes: &[u32]) -> Vec<usize> {
    let mut members = Vec::<(u32, Vec<usize>)>::new();
    for (place, &class) in classes.iter().enumerate() {
        match members.iter_mut().find(|(each, _)| *each == class) {
            Some((_, places)) => places.push(place),
            None => members.push((class, vec![place])),
        }
    }
    let mut ordered = Vec::with_capacity(classes.len());
    let mut left = Vec::new();
    for (_, places) in &members {
        let grouped = places.len() - places.len() % GROUP;
        ordered.extend_from_slice(&places[..grouped]);
        left.extend_from_slice(&places[grouped..]);
    }
    ordered.extend(left);
    ordered
}
