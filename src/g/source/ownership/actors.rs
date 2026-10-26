//! Compile source-prefix ownership into immutable actor decisions.
//!
//! Only this compiler reads the source tree or solves initial ownership. The
//! result refers to the source's canonical choices; it never stores a mutable
//! current-owner table or invents an independent decision authority.
//! These are binding facts, not operation admission: event activation and
//! ordering still belong to the projected local control/dependency facts.

use super::{
    Arm, At, Atom, Context, Error, ErrorKind, Evaluation, Owner, Query, Scope, Scratch, Selection,
    Source, error, validate,
};

#[path = "actors/knowledge.rs"]
mod knowledge;

#[path = "actors/reentry.rs"]
mod reentry;

#[cfg(kani)]
#[path = "actors/kani.rs"]
mod kani;

/// Actor values occupy the complete u8 domain. Larger references name a
/// previously emitted decision, leaving u16::MAX reserved for absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActorRef(pub(crate) u16);

impl ActorRef {
    const NODE_BASE: usize = u8::MAX as usize + 1;

    const fn actor(actor: u8) -> Self {
        Self(actor as u16)
    }

    const fn node(index: usize) -> Option<Self> {
        if index < u16::MAX as usize - Self::NODE_BASE {
            Some(Self((index + Self::NODE_BASE) as u16))
        } else {
            None
        }
    }

    const fn same(self, other: Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Clone, Copy)]
struct ActorChoice {
    scope: Scope,
    left: ActorRef,
    right: ActorRef,
}

impl ActorChoice {
    const EMPTY: Self = Self {
        scope: Scope {
            start: 0,
            split: 0,
            stop: 0,
        },
        left: ActorRef::actor(0),
        right: ActorRef::actor(0),
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ActorPair {
    pub(crate) sender: ActorRef,
    pub(crate) receiver: ActorRef,
}

impl ActorPair {
    const EMPTY: Self = Self {
        sender: ActorRef::actor(0),
        receiver: ActorRef::actor(0),
    };
}

/// Caller-supplied compiler storage. Events have two references into one
/// shared, reduced decision DAG. Neither source pointers nor protocol progress
/// are stored in this result. Final descriptor emission translates Scope ranges
/// to the corresponding compiled scope identities.
pub struct ActorImage<const EVENTS: usize, const DECISIONS: usize> {
    events: [ActorPair; EVENTS],
    choices: [ActorChoice; DECISIONS],
    event_len: usize,
    choice_len: usize,
}

impl<const E: usize, const D: usize> ActorImage<E, D> {
    /// Verifier-only DAG fixtures exercise the receive checker independently
    /// of ownership elaboration. All references are bounded and backward.
    #[cfg(any(kani, all(test, hibana_repo_tests, not(miri))))]
    pub(crate) const fn causal_fixture(
        bindings: &[[u16; 2]],
        decisions: &[(Scope, [u16; 2])],
    ) -> Self {
        assert!(bindings.len() <= E && decisions.len() <= D);
        let mut image = Self::new();
        let mut index = 0;
        while index < decisions.len() {
            let (scope, children) = decisions[index];
            assert!((children[0] as usize) < 256 + index);
            assert!((children[1] as usize) < 256 + index);
            image.choices[index] = ActorChoice {
                scope,
                left: ActorRef(children[0]),
                right: ActorRef(children[1]),
            };
            index += 1;
        }
        image.choice_len = decisions.len();
        index = 0;
        while index < bindings.len() {
            let pair = bindings[index];
            assert!((pair[0] as usize) < 256 + decisions.len());
            assert!((pair[1] as usize) < 256 + decisions.len());
            image.events[index] = ActorPair {
                sender: ActorRef(pair[0]),
                receiver: ActorRef(pair[1]),
            };
            index += 1;
        }
        image.event_len = bindings.len();
        image
    }

    pub const fn new() -> Self {
        Self {
            events: [ActorPair::EMPTY; E],
            choices: [ActorChoice::EMPTY; D],
            event_len: 0,
            choice_len: 0,
        }
    }

    pub const fn event_count(&self) -> usize {
        self.event_len
    }

    /// Check branch knowledge for this image's source. This construction fact
    /// does not certify route shape, framing, codecs or distributed execution.
    pub const fn has_branch_knowledge(&self, source: &Source<'_>) -> bool {
        knowledge::check(source, self)
    }

    /// Locate a rejected choice without changing its acceptance rule.
    pub(crate) const fn branch_knowledge_conflict(
        &self,
        source: &Source<'_>,
    ) -> Option<(Scope, Option<u8>, Option<usize>)> {
        knowledge::conflict(source, self)
    }

    /// A resource-changing continuation cannot leave a remote reentry sender
    /// able to initiate another visit without the entry owner's permission.
    pub const fn has_reentry_knowledge(&self, source: &Source<'_>) -> bool {
        reentry::check(source, self)
    }

    pub const fn decision_count(&self) -> usize {
        self.choice_len
    }

    pub const fn event_references(&self, event: usize) -> Option<[u16; 2]> {
        if event >= self.event_len {
            return None;
        }
        let pair = self.events[event];
        Some([pair.sender.0, pair.receiver.0])
    }

    pub(crate) const fn event_actors(&self) -> &[ActorPair] {
        self.events.split_at(self.event_len).0
    }

    /// Construction-time actor membership for lane coloring. Follow the same
    /// canonical decision DAG, retaining every possible leaf. This cannot select
    /// a current holder or authorize an operation. Backward references let each
    /// reachable node be processed once, without recursive expansion.
    pub(crate) const fn possible_actors(&self, event: usize) -> [u8; 32] {
        if event >= self.event_len {
            panic!("actor membership event outside source");
        }
        let pair = self.events[event];
        let roots = [pair.sender, pair.receiver];
        let mut actors = [0u8; 32];
        let mut pending = [false; D];
        let mut bound = 0;
        let mut side = 0;
        while side < 2 {
            let reference = roots[side].0 as usize;
            if reference < ActorRef::NODE_BASE {
                actors[reference / 8] |= 1 << (reference % 8);
            } else {
                let index = reference - ActorRef::NODE_BASE;
                if index >= self.choice_len {
                    panic!("actor membership reference outside source");
                }
                pending[index] = true;
                if index + 1 > bound {
                    bound = index + 1;
                }
            }
            side += 1;
        }
        while bound != 0 {
            bound -= 1;
            if pending[bound] {
                let choice = self.choices[bound];
                let children = [choice.left, choice.right];
                side = 0;
                while side < 2 {
                    let reference = children[side].0 as usize;
                    if reference < ActorRef::NODE_BASE {
                        actors[reference / 8] |= 1 << (reference % 8);
                    } else {
                        let index = reference - ActorRef::NODE_BASE;
                        if index >= bound {
                            panic!("actor membership reference is not backward");
                        }
                        pending[index] = true;
                    }
                    side += 1;
                }
            }
        }
        actors
    }

    pub const fn decision(&self, index: usize) -> Option<(Scope, [u16; 2])> {
        if index >= self.choice_len {
            return None;
        }
        let choice = self.choices[index];
        Some((choice.scope, [choice.left.0, choice.right.0]))
    }

    /// Read-only compiler/proof inspection, not runtime operation admission.
    /// Missing choices never choose a holder. A result is present only when
    /// every remaining possibility agrees, as with the source interpretation.
    pub const fn actors_at(&self, event: usize, selections: &[Selection]) -> Option<(u8, u8)> {
        if event >= self.event_len {
            return None;
        }
        let pair = self.events[event];
        let context = Context::new(selections);
        let Some(sender) = self.resolve(pair.sender, &context) else {
            return None;
        };
        let Some(receiver) = self.resolve(pair.receiver, &context) else {
            return None;
        };
        Some((sender, receiver))
    }

    const fn resolve(&self, reference: ActorRef, context: &Context<'_>) -> Option<u8> {
        if (reference.0 as usize) < ActorRef::NODE_BASE {
            return Some(reference.0 as u8);
        }
        let index = reference.0 as usize - ActorRef::NODE_BASE;
        if index >= self.choice_len {
            panic!("compiled actor decision reference");
        }
        let choice = self.choices[index];
        match context.get(choice.scope) {
            Some(Arm::Left) => self.resolve(choice.left, context),
            Some(Arm::Right) => self.resolve(choice.right, context),
            None => {
                let Some(left) = self.resolve(choice.left, context) else {
                    return None;
                };
                let Some(right) = self.resolve(choice.right, context) else {
                    return None;
                };
                if left == right { Some(left) } else { None }
            }
        }
    }

    const fn reset(&mut self) {
        self.event_len = 0;
        self.choice_len = 0;
    }

    const fn choice(
        &mut self,
        scope: Scope,
        left: ActorRef,
        right: ActorRef,
    ) -> Result<ActorRef, Error> {
        if left.same(right) {
            return Ok(left);
        }
        let mut index = 0;
        while index < self.choice_len {
            let choice = self.choices[index];
            if choice.scope.same(scope) && choice.left.same(left) && choice.right.same(right) {
                return match ActorRef::node(index) {
                    Some(reference) => Ok(reference),
                    None => Err(error(ErrorKind::Capacity, None)),
                };
            }
            index += 1;
        }
        if index == D {
            return Err(error(ErrorKind::Capacity, None));
        }
        let Some(reference) = ActorRef::node(index) else {
            return Err(error(ErrorKind::Capacity, None));
        };
        // Children were emitted first. Every non-actor reference therefore
        // points strictly backwards, so an accepted image cannot contain a cycle.
        self.choices[index] = ActorChoice { scope, left, right };
        self.choice_len += 1;
        Ok(reference)
    }

    const fn lower_actor<const S: usize>(
        &mut self,
        source: &Source<'_>,
        scratch: &Scratch<S, E>,
        at: At,
        context: &Context<'_>,
    ) -> Result<ActorRef, Error> {
        let scope = match Query::new(source).owner(at, context) {
            Evaluation::Atom(Atom::Actor(actor)) => return Ok(ActorRef::actor(actor)),
            Evaluation::Atom(Atom::Initial(key)) => {
                let Some(index) = scratch.index(key) else {
                    panic!("collected initial actor");
                };
                return match scratch.resolve(Owner::Initial(index as u16)) {
                    Some(actor) => Ok(ActorRef::actor(actor)),
                    None => Err(error(ErrorKind::UnrootedInitialOwner, Some(key))),
                };
            }
            Evaluation::Choice(scope) => scope,
        };
        let left_context = context.select(Selection {
            scope,
            arm: Arm::Left,
        });
        let left = match self.lower_actor(source, scratch, at, &left_context) {
            Ok(reference) => reference,
            Err(error) => return Err(error),
        };
        let right_context = context.select(Selection {
            scope,
            arm: Arm::Right,
        });
        let right = match self.lower_actor(source, scratch, at, &right_context) {
            Ok(reference) => reference,
            Err(error) => return Err(error),
        };
        self.choice(scope, left, right)
    }

    const fn emit<const S: usize>(
        &mut self,
        root: &Source<'_>,
        source: &Source<'_>,
        scratch: &Scratch<S, E>,
        cut: usize,
        context: &Context<'_>,
    ) -> Result<(), Error> {
        match source {
            Source::Send(message) => {
                if cut >= E {
                    return Err(error(ErrorKind::Capacity, None));
                }
                let sender = match self.lower_actor(
                    root,
                    scratch,
                    At {
                        key: message.from,
                        cut,
                    },
                    context,
                ) {
                    Ok(actor) => actor,
                    Err(error) => return Err(error),
                };
                let receiver = match self.lower_actor(
                    root,
                    scratch,
                    At {
                        key: message.to,
                        cut,
                    },
                    context,
                ) {
                    Ok(actor) => actor,
                    Err(error) => return Err(error),
                };
                self.events[cut] = ActorPair { sender, receiver };
                Ok(())
            }
            Source::Seq(left, right, _) | Source::Par(left, right, _) => {
                if let Err(error) = self.emit(root, left, scratch, cut, context) {
                    return Err(error);
                }
                self.emit(root, right, scratch, cut + left.event_count(), context)
            }
            Source::Route { left, right, .. } => {
                let split = cut + left.event_count();
                let scope = Scope {
                    start: cut as u16,
                    split: split as u16,
                    stop: (split + right.event_count()) as u16,
                };
                let left_context = context.select(Selection {
                    scope,
                    arm: Arm::Left,
                });
                if let Err(error) = self.emit(root, left, scratch, cut, &left_context) {
                    return Err(error);
                }
                let right_context = context.select(Selection {
                    scope,
                    arm: Arm::Right,
                });
                self.emit(root, right, scratch, split, &right_context)
            }
            Source::Roll(body, _) => self.emit(root, body, scratch, cut, context),
        }
    }
}

impl<const E: usize, const D: usize> Default for ActorImage<E, D> {
    fn default() -> Self {
        Self::new()
    }
}

/// Publish only a complete actor image. Source validation and all capacity
/// failures leave no accepted event or decision visible in the supplied output.
pub const fn compile_actor_image_into<const S: usize, const E: usize, const D: usize>(
    source: &Source<'_>,
    scratch: &mut Scratch<S, E>,
    image: &mut ActorImage<E, D>,
) -> Result<(), Error> {
    image.reset();
    if let Err(error) = validate(source, scratch) {
        return Err(error);
    }
    if let Err(error) = image.emit(source, source, scratch, 0, &Context::new(&[])) {
        image.reset();
        return Err(error);
    }
    image.event_len = scratch.event_len;
    Ok(())
}
