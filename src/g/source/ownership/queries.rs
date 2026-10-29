//! Source-prefix ownership queries. These run during projection
//! checking, never on the production runtime's hot path.
use super::{EndpointKey, Payload, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub start: u16,
    pub split: u16,
    pub stop: u16,
}

impl Scope {
    pub(super) const fn same(self, other: Self) -> bool {
        self.start == other.start && self.split == other.split && self.stop == other.stop
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub scope: Scope,
    pub arm: Arm,
}

/// A borrowed compile-time control path. No array of ownership maps is copied
/// when a branch is inspected. A choice always refers to its source scope.
pub(crate) struct Context<'a> {
    base: &'a [Selection],
    indexed: Option<&'a [Option<Selection>]>,
    extra: Option<(Selection, &'a Context<'a>)>,
}

impl<'a> Context<'a> {
    pub(crate) const fn new(base: &'a [Selection]) -> Self {
        Self {
            base,
            indexed: None,
            extra: None,
        }
    }

    /// Read-only compiler choices indexed by the source's binary split.
    pub(crate) const fn from_index(indexed: &'a [Option<Selection>]) -> Self {
        Self {
            base: &[],
            indexed: Some(indexed),
            extra: None,
        }
    }

    pub(crate) const fn select<'b>(&'b self, selection: Selection) -> Context<'b> {
        Context {
            base: self.base,
            indexed: self.indexed,
            extra: Some((selection, self)),
        }
    }

    pub(crate) const fn get(&self, scope: Scope) -> Option<Arm> {
        let mut context = self;
        while let Some((selection, parent)) = context.extra {
            if selection.scope.same(scope) {
                return Some(selection.arm);
            }
            context = parent;
        }
        if let Some(indexed) = context.indexed {
            if (scope.split as usize) < indexed.len()
                && let Some(selection) = indexed[scope.split as usize]
                && selection.scope.same(scope)
            {
                return Some(selection.arm);
            }
            return None;
        }
        let mut index = 0;
        while index < context.base.len() {
            if context.base[index].scope.same(scope) {
                return Some(context.base[index].arm);
            }
            index += 1;
        }
        None
    }
}

#[derive(Clone, Copy)]
pub(super) struct At {
    pub key: EndpointKey,
    pub cut: usize,
}

#[derive(Clone, Copy)]
pub(super) enum Atom {
    Actor(u8),
    Initial(EndpointKey),
}

#[derive(Clone, Copy)]
enum Value {
    Atom(Atom),
    Before(At),
    Choice { key: EndpointKey, scope: Scope },
}

pub(super) enum Evaluation {
    Atom(Atom),
    Choice(Scope),
}

pub(super) struct Query<'a> {
    source: &'a Source<'a>,
}

impl<'a> Query<'a> {
    pub(super) const fn new(source: &'a Source<'a>) -> Self {
        Self { source }
    }

    pub(super) const fn owner(&self, at: At, context: &Context<'_>) -> Evaluation {
        if !self.source.transfers(at.key) {
            return Evaluation::Atom(Atom::Actor(at.key.role));
        }
        let value = self.after(
            self.source,
            0,
            at,
            Value::Atom(Atom::Initial(at.key)),
            context,
        );
        self.evaluate(value, context)
    }

    const fn evaluate(&self, value: Value, context: &Context<'_>) -> Evaluation {
        match value {
            Value::Atom(atom) => Evaluation::Atom(atom),
            Value::Before(at) => self.owner(at, context),
            Value::Choice { key, scope } => match context.get(scope) {
                Some(_) => self.owner(
                    At {
                        key,
                        cut: scope.stop as usize,
                    },
                    context,
                ),
                None => Evaluation::Choice(scope),
            },
        }
    }

    const fn after(
        &self,
        source: &Source<'_>,
        start: usize,
        at: At,
        previous: Value,
        context: &Context<'_>,
    ) -> Value {
        if at.cut <= start || !source.transfers(at.key) {
            return previous;
        }
        match source {
            Source::Send(message) => {
                if let Payload::Endpoint(key) = message.payload
                    && key.same(at.key)
                {
                    // Capture the receiver's holder at this event. Moving the
                    // receiving endpoint later must not drag this value along.
                    Value::Before(At {
                        key: message.to,
                        cut: start,
                    })
                } else {
                    previous
                }
            }
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                let prior = self.after(left, start, at, previous, context);
                self.after(right, start + left.event_count(), at, prior, context)
            }
            Source::Route { left, right, .. } => {
                let split = start + left.event_count();
                let stop = split + right.event_count();
                let scope = Scope {
                    start: start as u16,
                    split: split as u16,
                    stop: stop as u16,
                };
                let selected = match context.get(scope) {
                    Some(arm) => Some(arm),
                    None if at.cut < stop => Some(if at.cut < split {
                        Arm::Left
                    } else {
                        Arm::Right
                    }),
                    None => None,
                };
                match selected {
                    Some(Arm::Left) => self.after(left, start, at, previous, context),
                    Some(Arm::Right) => self.after(right, split, at, previous, context),
                    None if source.transfers(at.key) => Value::Choice { key: at.key, scope },
                    None => previous,
                }
            }
            Source::Roll(body, _) => self.after(body, start, at, previous, context),
        }
    }
}
