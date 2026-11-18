//! Native input adaptation. Source names are evaluated in the source dictionary, then emitted as
//! exact-name native gates. No source ancestry mask is copied into a native dictionary.
use super::*;

#[derive(Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Tree<T> {
    Choice {
        value: T,
    },
    Input {
        builtin: u8,
        fallback: Box<Tree<T>>,
        cases: Vec<(u32, Tree<T>)>,
    },
}

impl<T> Tree<T> {
    pub fn map<U>(&self, transform: &mut impl FnMut(&T) -> U) -> Tree<U> {
        match self {
            Self::Choice { value } => Tree::Choice {
                value: transform(value),
            },
            Self::Input {
                builtin,
                fallback,
                cases,
            } => Tree::Input {
                builtin: *builtin,
                fallback: Box::new(fallback.map(transform)),
                cases: cases
                    .iter()
                    .map(|(name, tree)| (*name, tree.map(transform)))
                    .collect(),
            },
        }
    }

    pub fn leaves<'a>(&'a self, output: &mut Vec<&'a T>) {
        match self {
            Self::Choice { value } => output.push(value),
            Self::Input {
                fallback, cases, ..
            } => {
                fallback.leaves(output);
                for (_, tree) in cases {
                    tree.leaves(output);
                }
            }
        }
    }
}

impl<T: PartialEq> Tree<T> {
    pub fn reduced(self) -> Self {
        match self {
            Self::Choice { .. } => self,
            Self::Input {
                builtin,
                fallback,
                cases,
            } => {
                let fallback = Box::new(fallback.reduced());
                let cases = cases
                    .into_iter()
                    .map(|(name, tree)| (name, tree.reduced()))
                    .filter(|(_, tree)| tree != fallback.as_ref())
                    .collect::<Vec<_>>();
                if cases.is_empty() {
                    *fallback
                } else {
                    Self::Input {
                        builtin,
                        fallback,
                        cases,
                    }
                }
            }
        }
    }
}

/// Closed names written by Shadowkeep F676E0, F67880/F67950, F678E0 and F67790.
/// These are cross-version adaptations of source dictionary names, not recovered source setters.
pub(super) fn names(builtin: u8) -> Option<&'static [&'static str]> {
    match builtin {
        1 => Some(&["stand", "crouch"]),
        10 => Some(&["combat", "alert", "social"]),
        12 => Some(&["not_sprinting", "sprinting"]),
        18 => Some(&["empty", "partial", "full"]),
        _ => None,
    }
}

/// Enumerate only inputs read by either participating controller. Repeated requests with the
/// same clip but different local weapon motion remain distinct until their emitted choices agree.
pub(super) fn expand<T>(
    dimensions: &[u8],
    inputs: &mut BTreeMap<u8, u32>,
    leaf: &mut impl FnMut(&BTreeMap<u8, u32>) -> Result<T>,
) -> Result<Tree<T>> {
    crate::cancellation::check()?;
    let Some((&builtin, rest)) = dimensions.split_first() else {
        return Ok(Tree::Choice {
            value: leaf(inputs)?,
        });
    };
    ensure!(
        !inputs.contains_key(&builtin),
        "Dynamic animation input is already fixed"
    );
    let fallback = Box::new(expand(rest, inputs, leaf)?);
    let mut cases = Vec::new();
    for name in names(builtin).context("Animation input has no native producer adapter")? {
        let name = source::fnv(name);
        inputs.insert(builtin, name);
        cases.push((name, expand(rest, inputs, leaf)?));
        inputs.remove(&builtin);
    }
    Ok(Tree::Input {
        builtin,
        fallback,
        cases,
    })
}
