//! Route conditions from the typed source, shared by all role checks.
use crate::g::{ProgramSourceNode, SourceArm, SourceContext, SourceScope, SourceSelection};

#[derive(Clone, Copy)]
struct Condition {
    selection: SourceSelection,
    parent: u16,
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::g::{self, ProgramShape};

    #[test]
    fn cached_presence_matches_all_completions_of_each_partial_choice() {
        type Step = g::Send<0, 1, g::Msg<1, ()>>;
        type Steps = g::Seq<
            g::Route<g::Seq<Step, g::Route<Step, Step>>, g::Par<g::Route<Step, Step>, Step>>,
            g::Roll<Step>,
        >;
        let presence = SourcePresence::<32>::new(&Steps::SOURCE_NODE);
        let scopes = [
            SourceScope {
                start: 0,
                split: 3,
                stop: 6,
            },
            SourceScope {
                start: 1,
                split: 2,
                stop: 3,
            },
            SourceScope {
                start: 3,
                split: 4,
                stop: 5,
            },
        ];
        for partial in 0..27 {
            let mut choices = [0; 3];
            let mut remainder = partial;
            let mut selected = std::vec::Vec::new();
            for (position, scope) in scopes.iter().enumerate() {
                choices[position] = remainder % 3;
                remainder /= 3;
                if choices[position] != 0 {
                    selected.push(SourceSelection {
                        scope: *scope,
                        arm: if choices[position] == 1 {
                            SourceArm::Left
                        } else {
                            SourceArm::Right
                        },
                    });
                }
            }
            let path = SourceContext::new(&selected);
            for event in 0..7 {
                let mut completions = 0;
                let mut present = 0;
                for full in 0..8 {
                    let arms = [full & 1 == 0, full & 2 == 0, full & 4 == 0];
                    if choices
                        .iter()
                        .zip(arms)
                        .any(|(&choice, left)| choice != 0 && (choice == 1) != left)
                    {
                        continue;
                    }
                    completions += 1;
                    let occurs = match event {
                        0 => arms[0],
                        1 => arms[0] && arms[1],
                        2 => arms[0] && !arms[1],
                        3 => !arms[0] && arms[2],
                        4 => !arms[0] && !arms[2],
                        5 => !arms[0],
                        6 => true,
                        _ => unreachable!(),
                    };
                    present += usize::from(occurs);
                }
                match presence.active(event, &path) {
                    Ok(true) => assert_eq!(present, completions),
                    Ok(false) => assert_eq!(present, 0),
                    Err(scope) => {
                        assert!(present > 0 && present < completions);
                        assert!(path.get(scope).is_none());
                    }
                }
            }
        }
    }
}

pub(super) struct SourcePresence<const E: usize> {
    conditions: [Option<Condition>; E],
    heads: [u16; E],
}

impl<const E: usize> SourcePresence<E> {
    pub(super) const fn new(source: &ProgramSourceNode) -> Self {
        let mut index = Self {
            conditions: [None; E],
            heads: [u16::MAX; E],
        };
        let mut next = 0;
        index.collect(source, 0, u16::MAX, &mut next);
        index
    }

    const fn collect(
        &mut self,
        source: &ProgramSourceNode,
        start: usize,
        parent: u16,
        next: &mut usize,
    ) {
        match source {
            ProgramSourceNode::Send(_) => self.heads[start] = parent,
            ProgramSourceNode::Seq(left, right, _) | ProgramSourceNode::Par(left, right, _) => {
                self.collect(left, start, parent, next);
                self.collect(right, start + left.event_count(), parent, next);
            }
            ProgramSourceNode::Roll(body, _) => self.collect(body, start, parent, next),
            ProgramSourceNode::Route { left, right, .. } => {
                let split = start + left.event_count();
                let scope = SourceScope {
                    start: start as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                // Each route already needs four marker rows in E. Its two
                // condition links fit the same source-derived capacity.
                if *next + 2 > E || *next + 2 >= u16::MAX as usize {
                    crate::invariant();
                }
                let first = *next;
                *next += 2;
                self.conditions[first] = Some(Condition {
                    selection: SourceSelection {
                        scope,
                        arm: SourceArm::Left,
                    },
                    parent,
                });
                self.conditions[first + 1] = Some(Condition {
                    selection: SourceSelection {
                        scope,
                        arm: SourceArm::Right,
                    },
                    parent,
                });
                self.collect(left, start, first as u16, next);
                self.collect(right, split, (first + 1) as u16, next);
            }
        }
    }

    pub(super) const fn active(
        &self,
        event: usize,
        path: &SourceContext<'_>,
    ) -> Result<bool, SourceScope> {
        let mut link = self.heads[event];
        let mut missing = None;
        while link != u16::MAX {
            let Some(condition) = self.conditions[link as usize] else {
                crate::invariant();
            };
            match path.get(condition.selection.scope) {
                None => missing = Some(condition.selection.scope),
                Some(arm) if arm as u8 == condition.selection.arm as u8 => (),
                Some(_) => return Ok(false),
            }
            link = condition.parent;
        }
        match missing {
            Some(scope) => Err(scope),
            None => Ok(true),
        }
    }
}
