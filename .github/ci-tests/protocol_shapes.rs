//! Generate protocol syntax and replay inputs independently of projection.
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    path::PathBuf,
};

fn write_if_changed(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    match fs::read(path) {
        Ok(current) if current == text.as_bytes() => Ok(()),
        Ok(_) => fs::write(path, text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::write(path, text),
        Err(error) => Err(error),
    }
}

#[path = "protocol_shape_causality.rs"]
mod causality;
#[path = "protocol_shape_choice.rs"]
mod choice;
#[path = "protocol_shape_context.rs"]
mod context;
#[path = "protocol_shape_contracts.rs"]
mod contracts;
#[path = "protocol_shape_coverage.rs"]
mod coverage;
#[path = "protocol_shape_decisions.rs"]
mod decisions;
#[path = "protocol_shape_delegated.rs"]
mod delegated;
#[cfg(test)]
#[path = "protocol_shape_executions_tests.rs"]
mod execution_tests;
#[path = "protocol_shape_frontiers.rs"]
mod frontiers;
#[path = "protocol_shape_interactions.rs"]
mod interactions;
#[path = "protocol_shape_localside.rs"]
mod localside;
#[path = "protocol_shape_ownership.rs"]
mod ownership;
#[path = "protocol_shape_plans.rs"]
mod plans;
#[path = "protocol_shape_population.rs"]
mod population;
#[path = "protocol_shape_probe_data.rs"]
mod probe_data;
#[path = "protocol_shape_representatives.rs"]
mod representatives;
#[path = "protocol_shape_scope_probes.rs"]
mod scope_probes;
#[path = "protocol_shape_search.rs"]
mod search;
#[path = "protocol_shape_symmetry.rs"]
mod symmetry;
#[path = "protocol_shape_syntax.rs"]
mod syntax;
#[path = "protocol_shape_paths.rs"]
mod syntax_paths;
use population::{Count, Population};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Trace {
    messages: Vec<(u8, bool)>,
}

#[derive(Clone)]
enum Execution {
    Message(u8),
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>, bool),
    Branch(Box<Self>, bool),
    Roll(Box<Self>, Box<Self>),
}
impl Execution {
    fn trace(&self) -> Trace {
        match self {
            Self::Message(label) => Trace {
                messages: vec![(*label, false)],
            },
            Self::Seq(a, b) | Self::Roll(a, b) | Self::Par(a, b, false) => {
                a.trace().followed_by(&b.trace())
            }
            Self::Par(a, b, true) => b.trace().followed_by(&a.trace()),
            Self::Branch(body, _) => {
                let mut trace = body.trace();
                trace.messages.first_mut().unwrap().1 = true;
                trace
            }
        }
    }
    fn json(&self) -> String {
        self.numbered_json(&mut 0)
    }
    fn numbered_json(&self, next: &mut usize) -> String {
        match self {
            Self::Message(label) => {
                let occurrence = *next;
                *next += 1;
                format!("[0,{label},{occurrence}]")
            }
            Self::Seq(a, b) => format!("[1,{},{}]", a.numbered_json(next), b.numbered_json(next)),
            Self::Par(a, b, reverse) => {
                format!(
                    "[2,{},{},{reverse}]",
                    a.numbered_json(next),
                    b.numbered_json(next)
                )
            }
            Self::Branch(body, right) => {
                format!("[3,{},{right}]", body.numbered_json(next))
            }
            Self::Roll(a, b) => format!("[4,{},{}]", a.numbered_json(next), b.numbered_json(next)),
        }
    }
    fn message_count(&self) -> usize {
        match self {
            Self::Message(_) => 1,
            Self::Seq(a, b) | Self::Par(a, b, _) | Self::Roll(a, b) => {
                a.message_count() + b.message_count()
            }
            Self::Branch(body, _) => body.message_count(),
        }
    }
}
impl Trace {
    fn followed_by(&self, other: &Self) -> Self {
        let mut combined = self.clone();
        combined.messages.extend(&other.messages);
        combined
    }
    fn line(&self) -> String {
        let labels = self
            .messages
            .iter()
            .map(|(label, preview)| format!("{label}{}", if *preview { "?" } else { "" }))
            .collect::<Vec<_>>()
            .join(",");
        format!("{labels}\n")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Protocol {
    Message {
        from: u8,
        to: u8,
        event: u8,
        contract: u8,
        owned: bool,
    },
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>, u16),
    Roll(Box<Self>),
}
impl Protocol {
    fn contract_for(&self, event: u8) -> u8 {
        match self {
            Self::Message {
                event: current,
                contract,
                ..
            } if *current == event => *contract,
            Self::Message { .. } => panic!("unknown source event"),
            Self::Roll(body) => body.contract_for(event),
            Self::Seq(left, right)
            | Self::Par(left, right)
            | Self::Route(left, right)
            | Self::Resolved(left, right, _) => {
                if localside::contains(left, event) {
                    left.contract_for(event)
                } else {
                    right.contract_for(event)
                }
            }
        }
    }

    fn syntax_depth(&self) -> usize {
        match self {
            Self::Message { .. } => 0,
            Self::Roll(body) => 1 + body.syntax_depth(),
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                1 + a.syntax_depth().max(b.syntax_depth())
            }
        }
    }

    fn operator_count(&self) -> usize {
        match self {
            Self::Message { .. } => 0,
            Self::Roll(body) => 1 + body.operator_count(),
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                1 + a.operator_count() + b.operator_count()
            }
        }
    }

    fn rust(&self) -> String {
        match self {
            Self::Message {
                from,
                to,
                event: label,
                ..
            } => format!("g::send::<{from}, {to}, M{label}<'_>>()"),
            Self::Seq(a, b) => format!("g::seq({}, {})", a.rust(), b.rust()),
            Self::Par(a, b) => format!("g::par({}, {})", a.rust(), b.rust()),
            Self::Route(a, b) => format!("g::route({}, {})", a.rust(), b.rust()),
            Self::Resolved(a, b, id) => {
                format!("g::route({}, {}).resolve::<{id}>()", a.rust(), b.rust())
            }
            Self::Roll(a) => format!("({}).roll()", a.rust()),
        }
    }
    fn json_nodes(&self, out: &mut Vec<String>) -> usize {
        let row = match self {
            Self::Message {
                from,
                to,
                event: label,
                contract,
                owned,
                ..
            } => format!(
                r#"{{"kind":0,"left":0,"right":0,"sender":{from},"receiver":{to},"event":{label},"label":{contract},"owned":{owned},"resolver":0}}"#
            ),
            Self::Roll(body) => {
                let left = body.json_nodes(out);
                format!(
                    r#"{{"kind":5,"left":{left},"right":0,"sender":0,"receiver":0,"event":0,"label":0,"owned":false,"resolver":0}}"#
                )
            }
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                let left = a.json_nodes(out);
                let right = b.json_nodes(out);
                let (kind, resolver) = match self {
                    Self::Seq(..) => (1, 0),
                    Self::Par(..) => (2, 0),
                    Self::Route(..) => (3, 0),
                    Self::Resolved(_, _, id) => (4, *id),
                    _ => unreachable!(),
                };
                format!(
                    r#"{{"kind":{kind},"left":{left},"right":{right},"sender":0,"receiver":0,"event":0,"label":0,"owned":false,"resolver":{resolver}}}"#
                )
            }
        };
        let index = out.len();
        out.push(row);
        index
    }
    fn swap_participants(&mut self) {
        match self {
            Self::Message { from, to, .. } => {
                if *from < 2 {
                    *from ^= 1;
                }
                if *to < 2 {
                    *to ^= 1;
                }
            }
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                a.swap_participants();
                b.swap_participants();
            }
            Self::Roll(a) => a.swap_participants(),
        }
    }
    fn messages<'a>(&'a self, out: &mut Vec<&'a Self>) {
        match self {
            Self::Message { .. } => out.push(self),
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                a.messages(out);
                b.messages(out);
            }
            Self::Roll(a) => a.messages(out),
        }
    }
    fn repetition_count(&self) -> usize {
        match self {
            Self::Message { .. } => 0,
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) | Self::Resolved(a, b, _) => {
                a.repetition_count() + b.repetition_count()
            }
            Self::Roll(body) => 1 + body.repetition_count(),
        }
    }
    fn has_one_sided_choice(&self, role: u8) -> bool {
        match self {
            Self::Message { .. } => false,
            Self::Seq(a, b) | Self::Par(a, b) => {
                a.has_one_sided_choice(role) || b.has_one_sided_choice(role)
            }
            Self::Route(a, b) | Self::Resolved(a, b, _) => {
                localside::present(a, role, None) != localside::present(b, role, None)
                    || a.has_one_sided_choice(role)
                    || b.has_one_sided_choice(role)
            }
            Self::Roll(body) => body.has_one_sided_choice(role),
        }
    }
    fn prefix_visits(&self) -> usize {
        match self {
            Self::Roll(body) if body.repetition_count() == 0 => 2,
            _ if self.repetition_count() == 0 => 1,
            _ => 0,
        }
    }
    fn resolver_choices(
        &self,
        path: &mut Vec<(u16, bool)>,
        out: &mut BTreeMap<u8, Vec<(u16, bool)>>,
    ) {
        match self {
            Self::Message { event: label, .. } => {
                assert!(out.insert(*label, path.clone()).is_none());
            }
            Self::Resolved(a, b, id) => {
                path.push((*id, false));
                a.resolver_choices(path, out);
                path.last_mut().unwrap().1 = true;
                b.resolver_choices(path, out);
                path.pop();
            }
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) => {
                a.resolver_choices(path, out);
                b.resolver_choices(path, out);
            }
            Self::Roll(body) => body.resolver_choices(path, out),
        }
    }

    fn resolver_roles(&self, roles: &mut Vec<u8>) {
        match self {
            Self::Message { .. } => {}
            Self::Resolved(a, b, id) => {
                let mut head = a.as_ref();
                while let Self::Seq(first, _)
                | Self::Par(first, _)
                | Self::Route(first, _)
                | Self::Resolved(first, _, _)
                | Self::Roll(first) = head
                {
                    head = first;
                }
                let Self::Message { from, .. } = head else {
                    unreachable!()
                };
                assert_eq!(usize::from(*id), roles.len());
                roles.push(*from);
                a.resolver_roles(roles);
                b.resolver_roles(roles);
            }
            Self::Seq(a, b) | Self::Par(a, b) | Self::Route(a, b) => {
                a.resolver_roles(roles);
                b.resolver_roles(roles);
            }
            Self::Roll(body) => body.resolver_roles(roles),
        }
    }
    // Retain each visit and parallel boundary even when two executions have
    // the same message word. The public borrow API observes these boundaries.
    fn execution_count(&self) -> usize {
        match self {
            Self::Message { .. } => Some(1),
            Self::Seq(a, b) => a.execution_count().checked_mul(b.execution_count()),
            Self::Par(a, b) => a
                .execution_count()
                .checked_mul(b.execution_count())
                .and_then(|count| count.checked_mul(2)),
            Self::Route(a, b) | Self::Resolved(a, b, _) => {
                a.execution_count().checked_add(b.execution_count())
            }
            Self::Roll(body) => {
                let count = body.execution_count();
                count.checked_mul(count)
            }
        }
        .expect("execution count exceeds the host usize domain")
    }

    fn execution_at(&self, rank: usize) -> Execution {
        match self {
            Self::Message { event: label, .. } => {
                assert_eq!(rank, 0);
                Execution::Message(*label)
            }
            Self::Seq(a, b) | Self::Par(a, b) => {
                let pair = if matches!(self, Self::Par(..)) {
                    rank / 2
                } else {
                    rank
                };
                let width = b.execution_count();
                let left = Box::new(a.execution_at(pair / width));
                let right = Box::new(b.execution_at(pair % width));
                if matches!(self, Self::Par(..)) {
                    Execution::Par(left, right, !rank.is_multiple_of(2))
                } else {
                    Execution::Seq(left, right)
                }
            }
            Self::Route(a, b) | Self::Resolved(a, b, _) => {
                let width = a.execution_count();
                if rank < width {
                    Execution::Branch(Box::new(a.execution_at(rank)), false)
                } else {
                    Execution::Branch(Box::new(b.execution_at(rank - width)), true)
                }
            }
            Self::Roll(body) => {
                let width = body.execution_count();
                Execution::Roll(
                    Box::new(body.execution_at(rank / width)),
                    Box::new(body.execution_at(rank % width)),
                )
            }
        }
    }

    #[cfg(test)]
    fn executions(&self) -> impl ExactSizeIterator<Item = Execution> + '_ {
        (0..self.execution_count()).map(|rank| self.execution_at(rank))
    }
}

fn message(next: &mut u8, from: u8, to: u8, owned: bool) -> Protocol {
    let label = *next;
    *next = next.checked_add(1).expect("message-label domain");
    Protocol::Message {
        from,
        to,
        event: label,
        owned,
        contract: label,
    }
}
// Every tree with at most `budget` operator nodes is enumerated. The leaf
// performs a complete affine loan; seq/par add one ordinary message and route
// may have independently constructed left and right children.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Shape {
    Loan,
    Seq(Box<Self>),
    Par(Box<Self>),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>),
    Roll(Box<Self>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum OrdinaryLeaf {
    Message,
    Exchange,
    Converging,
}
impl Shape {
    fn ordinary(
        &self,
        next: &mut u8,
        resolver: &mut u16,
        service: u8,
        leaf: OrdinaryLeaf,
    ) -> Protocol {
        let peer = match leaf {
            OrdinaryLeaf::Message => {
                if service == 0 {
                    1
                } else {
                    service
                }
            }
            OrdinaryLeaf::Exchange | OrdinaryLeaf::Converging => 1,
        };
        match self {
            Self::Loan => match leaf {
                OrdinaryLeaf::Message => message(next, 0, peer, false),
                OrdinaryLeaf::Exchange => Protocol::Seq(
                    Box::new(message(next, 0, 1, false)),
                    Box::new(Protocol::Seq(
                        Box::new(message(next, 1, service, false)),
                        Box::new(Protocol::Seq(
                            Box::new(message(next, service, 1, false)),
                            Box::new(message(next, 1, 0, false)),
                        )),
                    )),
                ),
                OrdinaryLeaf::Converging => Protocol::Seq(
                    Box::new(message(next, 0, 1, false)),
                    Box::new(Protocol::Seq(
                        Box::new(message(next, 0, 2, false)),
                        Box::new(message(next, 1, 2, false)),
                    )),
                ),
            },
            Self::Seq(child) => Protocol::Seq(
                Box::new(message(next, 0, peer, false)),
                Box::new(child.ordinary(next, resolver, service, leaf)),
            ),
            Self::Par(child) => Protocol::Par(
                Box::new(child.ordinary(next, resolver, service, leaf)),
                Box::new(message(
                    next,
                    0,
                    if service == 0 { 1 } else { service },
                    false,
                )),
            ),
            Self::Route(a, b) => Protocol::Route(
                Box::new(a.ordinary(next, resolver, service, leaf)),
                Box::new(b.ordinary(next, resolver, service, leaf)),
            ),
            Self::Resolved(a, b) => {
                let id = *resolver;
                *resolver += 1;
                Protocol::Resolved(
                    Box::new(a.ordinary(next, resolver, service, leaf)),
                    Box::new(b.ordinary(next, resolver, service, leaf)),
                    id,
                )
            }
            Self::Roll(child) => {
                Protocol::Roll(Box::new(child.ordinary(next, resolver, service, leaf)))
            }
        }
    }
    fn cost(&self) -> usize {
        match self {
            Self::Loan => 0,
            Self::Seq(a) | Self::Par(a) | Self::Roll(a) => 1 + a.cost(),
            Self::Route(a, b) | Self::Resolved(a, b) => 1 + a.cost() + b.cost(),
        }
    }
    fn leaves(&self) -> usize {
        match self {
            Self::Loan => 1,
            Self::Seq(child) | Self::Par(child) | Self::Roll(child) => child.leaves(),
            Self::Route(a, b) | Self::Resolved(a, b) => a.leaves() + b.leaves(),
        }
    }
    fn build(&self, next: &mut u8, resolver: &mut u16, service: u8) -> Protocol {
        self.build_with(next, resolver, service, &mut |next, _| loan(next, service))
    }
    fn build_with(
        &self,
        next: &mut u8,
        resolver: &mut u16,
        service: u8,
        leaf: &mut impl FnMut(&mut u8, &mut u16) -> Protocol,
    ) -> Protocol {
        match self {
            Self::Loan => leaf(next, resolver),
            Self::Seq(child) => {
                let prefix = message(next, 0, 1, false);
                Protocol::Seq(
                    Box::new(prefix),
                    Box::new(child.build_with(next, resolver, service, leaf)),
                )
            }
            Self::Par(child) => {
                let a = child.build_with(next, resolver, service, leaf);
                let b = message(next, 0, if service == 0 { 1 } else { service }, false);
                Protocol::Par(Box::new(a), Box::new(b))
            }
            Self::Route(a, b) => Protocol::Route(
                Box::new(a.build_with(next, resolver, service, leaf)),
                Box::new(b.build_with(next, resolver, service, leaf)),
            ),
            Self::Resolved(a, b) => {
                let id = *resolver;
                *resolver += 1;
                Protocol::Resolved(
                    Box::new(a.build_with(next, resolver, service, leaf)),
                    Box::new(b.build_with(next, resolver, service, leaf)),
                    id,
                )
            }
            Self::Roll(child) => {
                Protocol::Roll(Box::new(child.build_with(next, resolver, service, leaf)))
            }
        }
    }
}
fn loan(next: &mut u8, service: u8) -> Protocol {
    let a = message(next, 0, 1, true);
    let b = message(next, 3, service, false);
    let c = message(next, service, 3, false);
    let d = message(next, 1, 0, true);
    Protocol::Seq(
        Box::new(a),
        Box::new(Protocol::Seq(
            Box::new(b),
            Box::new(Protocol::Seq(Box::new(c), Box::new(d))),
        )),
    )
}
fn shapes(budget: usize) -> BTreeSet<Shape> {
    let mut levels = vec![vec![Shape::Loan]];
    for count in 1..=budget {
        let mut next = Vec::new();
        for child in &levels[count - 1] {
            next.push(Shape::Seq(Box::new(child.clone())));
            next.push(Shape::Par(Box::new(child.clone())));
            next.push(Shape::Roll(Box::new(child.clone())));
        }
        for left_count in 0..count {
            for left in &levels[left_count] {
                for right in &levels[count - 1 - left_count] {
                    next.push(Shape::Route(
                        Box::new(left.clone()),
                        Box::new(right.clone()),
                    ));
                    next.push(Shape::Resolved(
                        Box::new(left.clone()),
                        Box::new(right.clone()),
                    ));
                }
            }
        }
        levels.push(next);
    }
    levels.into_iter().flatten().collect()
}

// One complete resource loan is the leaf. Every binary constructor can have
// independently generated children; a parallel child uses ordinary exchanges
// so the same resource is never lent along two independent paths.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BinaryShape {
    Leaf(OrdinaryLeaf),
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>, bool),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>),
    Roll(Box<Self>),
    Receive(Box<Self>),
}

struct BinaryCounts {
    resource: Vec<Count>,
    ordinary: Vec<Count>,
}
impl BinaryCounts {
    fn for_kind(&self, owned: bool) -> &[Count] {
        if owned {
            &self.resource
        } else {
            &self.ordinary
        }
    }
    fn total(&self, owned: bool, population: Population) -> usize {
        self.for_kind(owned)
            .iter()
            .map(|count| count.get(population))
            .sum()
    }

    fn shape(&self, mut rank: usize, owned: bool, population: Population) -> BinaryShape {
        let rows = self.for_kind(owned);
        let mut cost = 0;
        while rank >= rows[cost].get(population) {
            rank -= rows[cost].get(population);
            cost += 1;
        }
        BinaryShape::at(cost, rank, self, owned, population)
    }
}
fn binary_counts(budget: usize) -> BinaryCounts {
    let mut counts = BinaryCounts {
        resource: vec![Count {
            exchange: 1,
            any: 1,
        }],
        ordinary: vec![Count {
            exchange: 1,
            any: 2,
        }],
    };
    for cost in 1..=budget {
        let mut resource = 2 * counts.resource[cost - 1];
        let mut ordinary = 2 * counts.ordinary[cost - 1];
        for left in 0..cost {
            let right = cost - 1 - left;
            resource = resource
                + 3 * counts.resource[left] * counts.resource[right]
                + counts.resource[left] * counts.ordinary[right]
                + counts.ordinary[left] * counts.resource[right];
            ordinary = ordinary + 4 * counts.ordinary[left] * counts.ordinary[right];
        }
        counts.resource.push(resource);
        counts.ordinary.push(ordinary);
    }
    counts
}

impl BinaryShape {
    fn at(
        cost: usize,
        mut index: usize,
        counts: &BinaryCounts,
        owned: bool,
        population: Population,
    ) -> Self {
        assert!(index < counts.for_kind(owned)[cost].get(population));
        if cost == 0 {
            return Self::Leaf(match (population, index) {
                (Population::Exchange | Population::Any, 0) => OrdinaryLeaf::Exchange,
                (Population::Converging, 0) | (Population::Any, 1) => OrdinaryLeaf::Converging,
                _ => unreachable!("binary leaf rank"),
            });
        }
        let unary = counts.for_kind(owned)[cost - 1].get(population);
        for operator in 0..2 {
            if index < unary {
                let child = Box::new(Self::at(cost - 1, index, counts, owned, population));
                return match operator {
                    0 => Self::Roll(child),
                    1 => Self::Receive(child),
                    _ => unreachable!(),
                };
            }
            index -= unary;
        }
        for left_cost in 0..cost {
            let right_cost = cost - 1 - left_cost;
            for operator in 0..5 {
                if !owned && operator == 2 {
                    continue;
                }
                let left_owned = owned && operator != 2;
                let right_owned = owned && operator != 1;
                for &(left_population, right_population) in population.children() {
                    let rights = counts.for_kind(right_owned)[right_cost].get(right_population);
                    let pairs =
                        counts.for_kind(left_owned)[left_cost].get(left_population) * rights;
                    if index < pairs {
                        let left = Box::new(Self::at(
                            left_cost,
                            index / rights,
                            counts,
                            left_owned,
                            left_population,
                        ));
                        let right = Box::new(Self::at(
                            right_cost,
                            index % rights,
                            counts,
                            right_owned,
                            right_population,
                        ));
                        return match operator {
                            0 => Self::Seq(left, right),
                            1 => Self::Par(left, right, false),
                            2 => Self::Par(left, right, true),
                            3 => Self::Route(left, right),
                            4 => Self::Resolved(left, right),
                            _ => unreachable!(),
                        };
                    }
                    index -= pairs;
                }
            }
        }
        unreachable!("binary shape index");
    }

    fn build(&self, next: &mut u8, resolver: &mut u16, service: u8, owned: bool) -> Protocol {
        match self {
            Self::Leaf(_) if owned => loan(next, service),
            Self::Leaf(leaf) => Shape::Loan.ordinary(next, resolver, service, *leaf),
            Self::Seq(a, b) => Protocol::Seq(
                Box::new(a.build(next, resolver, service, owned)),
                Box::new(b.build(next, resolver, service, owned)),
            ),
            Self::Par(a, b, reversed) => Protocol::Par(
                Box::new(a.build(next, resolver, service, owned && !reversed)),
                Box::new(b.build(next, resolver, service, owned && *reversed)),
            ),
            Self::Route(a, b) => Protocol::Route(
                Box::new(a.build(next, resolver, service, owned)),
                Box::new(b.build(next, resolver, service, owned)),
            ),
            Self::Resolved(a, b) => {
                let id = *resolver;
                *resolver += 1;
                Protocol::Resolved(
                    Box::new(a.build(next, resolver, service, owned)),
                    Box::new(b.build(next, resolver, service, owned)),
                    id,
                )
            }
            Self::Roll(body) => {
                Protocol::Roll(Box::new(body.build(next, resolver, service, owned)))
            }
            Self::Receive(body) => Protocol::Seq(
                Box::new(message(next, 0, 1, false)),
                Box::new(Protocol::Seq(
                    Box::new(message(next, 1, 0, false)),
                    Box::new(body.build(next, resolver, service, owned)),
                )),
            ),
        }
    }
}

#[derive(Clone, Debug)]
enum CaseShape {
    Delegated(delegated::Body),
    Binary(BinaryShape),
    OrdinaryBinary(BinaryShape),
    Owned(Shape),
    Incoming(Shape),
    Mixed(Shape, Shape, bool, OrdinaryLeaf),
    MixedIncoming(Shape, Shape, bool, OrdinaryLeaf),
    NestedParallel {
        context: Shape,
        site: usize,
        resource: Shape,
        ordinary: Shape,
        reversed: bool,
        leaf: OrdinaryLeaf,
    },
}
impl CaseShape {
    fn build(&self, next: &mut u8, resolvers: &mut u16, service: u8) -> Protocol {
        match self {
            Self::Delegated(body) => body.global(next, resolvers),
            Self::Binary(shape) => shape.build(next, resolvers, service, true),
            Self::OrdinaryBinary(shape) => shape.build(next, resolvers, service, false),
            Self::NestedParallel {
                context,
                site,
                resource,
                ordinary,
                reversed,
                leaf,
            } => {
                let mut position = 0;
                let global =
                    context.build_with(next, resolvers, service, &mut |next, resolvers| {
                        let current = position;
                        position += 1;
                        if current != *site {
                            return loan(next, service);
                        }
                        if *reversed {
                            let a = ordinary.ordinary(next, resolvers, service, *leaf);
                            let b = resource.build(next, resolvers, service);
                            Protocol::Par(Box::new(a), Box::new(b))
                        } else {
                            let a = resource.build(next, resolvers, service);
                            let b = ordinary.ordinary(next, resolvers, service, *leaf);
                            Protocol::Par(Box::new(a), Box::new(b))
                        }
                    });
                assert!(*site < position);
                global
            }
            Self::Owned(shape) => shape.build(next, resolvers, service),
            Self::Incoming(shape) => Protocol::Seq(
                Box::new(message(next, 1, 0, false)),
                Box::new(shape.build(next, resolvers, service)),
            ),
            Self::Mixed(resource, ordinary, reversed, leaf)
            | Self::MixedIncoming(resource, ordinary, reversed, leaf) => {
                let incoming = matches!(self, Self::MixedIncoming(..));
                let build_resource = |next: &mut u8, resolvers: &mut u16| {
                    if incoming {
                        Protocol::Seq(
                            Box::new(message(next, 1, 0, false)),
                            Box::new(resource.build(next, resolvers, service)),
                        )
                    } else {
                        resource.build(next, resolvers, service)
                    }
                };
                if *reversed {
                    let a = ordinary.ordinary(next, resolvers, service, *leaf);
                    let b = build_resource(next, resolvers);
                    Protocol::Par(Box::new(a), Box::new(b))
                } else {
                    let a = build_resource(next, resolvers);
                    let b = ordinary.ordinary(next, resolvers, service, *leaf);
                    Protocol::Par(Box::new(a), Box::new(b))
                }
            }
        }
    }
}
fn catalog(budget: usize) -> Vec<CaseShape> {
    let mut cases = shapes(budget)
        .into_iter()
        .map(CaseShape::Owned)
        .collect::<Vec<_>>();
    if budget == 0 {
        return cases;
    }
    let mut levels = vec![Vec::new(); budget];
    for shape in shapes(budget - 1) {
        cases.push(CaseShape::Incoming(shape.clone()));
        levels[shape.cost()].push(shape);
    }
    for cost in 0..budget {
        for resource_cost in 0..=cost {
            for resource in &levels[resource_cost] {
                for ordinary in &levels[cost - resource_cost] {
                    for reversed in [false, true] {
                        for leaf in [OrdinaryLeaf::Message, OrdinaryLeaf::Exchange] {
                            cases.push(CaseShape::Mixed(
                                resource.clone(),
                                ordinary.clone(),
                                reversed,
                                leaf,
                            ));
                            if cost + 2 <= budget {
                                cases.push(CaseShape::MixedIncoming(
                                    resource.clone(),
                                    ordinary.clone(),
                                    reversed,
                                    leaf,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    for context_cost in 1..budget {
        for resource_cost in 0..budget - context_cost {
            for ordinary_cost in 0..budget - context_cost - resource_cost {
                for context in &levels[context_cost] {
                    for site in 0..context.leaves() {
                        for resource in &levels[resource_cost] {
                            for ordinary in &levels[ordinary_cost] {
                                for reversed in [false, true] {
                                    for leaf in [OrdinaryLeaf::Message, OrdinaryLeaf::Exchange] {
                                        cases.push(CaseShape::NestedParallel {
                                            context: context.clone(),
                                            site,
                                            resource: resource.clone(),
                                            ordinary: ordinary.clone(),
                                            reversed,
                                            leaf,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    cases
}
type Case = (usize, String, u8, u8, Protocol, u16);
#[derive(Clone, Copy)]
enum CatalogOrigin {
    Templates,
    Syntax,
    Representatives,
    Contexts,
}

impl CatalogOrigin {
    fn requires_diagnostics(self) -> bool {
        matches!(self, Self::Syntax | Self::Representatives | Self::Contexts)
    }

    fn compilation_width(self) -> usize {
        match self {
            Self::Syntax => 1,
            Self::Contexts => 8,
            Self::Templates | Self::Representatives => 32,
        }
    }
}

struct CaseSet {
    namespace: String,
    description: String,
    total: String,
    cases: Vec<Case>,
    delegated: BTreeSet<usize>,
    expected_rejections: BTreeMap<usize, &'static str>,
    negative_sources: BTreeMap<String, (Protocol, &'static str)>,
    max_label: u16,
    max_resolvers: u16,
    origin: CatalogOrigin,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|s| s == "syntax-output") {
        if args.len() != 4 {
            return Err("usage: protocol-shapes syntax-output DEPTH FIRST COUNT".into());
        }
        let depth = args[1].parse::<usize>()?;
        let first = args[2].parse::<num_bigint::BigUint>()?;
        let count = args[3].parse::<usize>()?;
        syntax_matrix(depth, first.clone(), count)?;
        println!("{}", syntax_paths::relative(depth, &first, count).display());
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "syntax-sample") {
        if args.len() != 5 {
            return Err("usage: protocol-shapes syntax-sample DEPTH SEED COUNT OUTPUT_TSV".into());
        }
        return search::sample(
            args[1].parse()?,
            args[2].parse()?,
            args[3].parse()?,
            std::path::Path::new(&args[4]),
        );
    }
    if args.first().is_some_and(|s| s == "syntax-shrink") {
        if args.len() < 7 || args[5] != "--" {
            return Err("usage: protocol-shapes syntax-shrink REPOSITORY DEPTH RANK OUTPUT -- PROBE [ARG...]".into());
        }
        return search::shrink(
            std::path::Path::new(&args[1]),
            args[2].parse()?,
            args[3].parse()?,
            std::path::Path::new(&args[4]),
            &args[6..],
        );
    }
    if args.first().is_some_and(|s| s == "syntax-probe") {
        if args.len() != 5 {
            return Err("usage: protocol-shapes syntax-probe REPOSITORY DEPTH RANK OUTPUT".into());
        }
        std::process::exit(search::probe(
            std::path::Path::new(&args[1]),
            args[2].parse()?,
            args[3].parse()?,
            std::path::Path::new(&args[4]),
        )?);
    }
    if args.first().is_some_and(|s| s == "syntax-fuzz") {
        if args.len() != 6 {
            return Err(
                "usage: protocol-shapes syntax-fuzz REPOSITORY DEPTH SEED COUNT OUTPUT".into(),
            );
        }
        return search::fuzz(
            std::path::Path::new(&args[1]),
            args[2].parse()?,
            args[3].parse()?,
            args[4].parse()?,
            std::path::Path::new(&args[5]),
        );
    }
    if args.first().is_some_and(|s| s == "coverage-uncovered") {
        if args.len() < 3 {
            return Err("usage: protocol-shapes coverage-uncovered LENGTH OUTPUT_TSV [SUCCESSFUL_RUN_DIRECTORY...]".into());
        }
        return coverage::uncovered(
            args[1].parse()?,
            std::path::Path::new(&args[2]),
            &args[3..].iter().map(PathBuf::from).collect::<Vec<_>>(),
        );
    }
    if args.first().is_some_and(|s| s == "representative-counts") {
        if args.len() != 2 && args.len() != 3 {
            return Err("usage: protocol-shapes representative-counts DEPTH [RANKS_TSV]".into());
        }
        let depth = args[1].parse::<usize>()?;
        let ranks = representatives::catalog(depth)?;
        println!("depth={depth} canonical_source_candidates={}", ranks.len());
        if let Some(path) = args.get(2) {
            use std::io::Write as _;
            let mut output = std::io::BufWriter::new(fs::File::create(path)?);
            for (ordinal, rank) in ranks.iter().enumerate() {
                writeln!(output, "{ordinal}\t{rank}")?;
            }
            output.flush()?;
        }
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "syntax-counts") {
        if args.len() != 2 {
            return Err("usage: protocol-shapes syntax-counts DEPTH".into());
        }
        let depth = args[1].parse::<usize>()?;
        if depth > 8 {
            return Err("syntax count depth must be at most eight".into());
        }
        println!(
            "depth={depth} raw={} role_orbits={}",
            syntax::Grammar::new(depth).count(depth),
            symmetry::count(depth)
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "representative-matrix") {
        if args.len() != 4 {
            return Err("usage: protocol-shapes representative-matrix DEPTH FIRST COUNT".into());
        }
        let ranks = representatives::catalog(args[1].parse()?)?;
        println!(
            "{}",
            range_matrix(args[2].parse()?, args[3].parse()?, ranks.len().into())?
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "syntax-matrix") {
        if args.len() != 4 {
            return Err("usage: protocol-shapes syntax-matrix DEPTH FIRST COUNT".into());
        }
        println!(
            "{}",
            syntax_matrix(args[1].parse()?, args[2].parse()?, args[3].parse()?)?
        );
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|s| s == "interaction-matrix" || s == "spine-matrix")
    {
        if args.len() != 4 {
            return Err("usage: protocol-shapes interaction-matrix LENGTH FIRST COUNT".into());
        }
        let length = args[1].parse::<usize>()?;
        if length > 5 {
            return Err("interaction length must be at most five".into());
        }
        println!(
            "{}",
            context_matrix(
                length,
                args[2].parse()?,
                args[3].parse()?,
                args[0] == "spine-matrix"
            )?
        );
        return Ok(());
    }
    let (catalog, diagnostics) = if args.get(2).is_some_and(|s| s == "--contracts") {
        if args.len() != 5 && args.len() != 6 {
            return Err(
                "usage: protocol-shapes REPOSITORY OUTPUT --contracts FIRST COUNT [DIAGNOSTICS]"
                    .into(),
            );
        }
        (
            contract_collision_cases(args[3].parse()?, args[4].parse()?)?,
            args.get(5),
        )
    } else if args.get(2).is_some_and(|s| s == "--frontiers") {
        if args.len() != 5 && args.len() != 6 {
            return Err(
                "usage: protocol-shapes REPOSITORY OUTPUT --frontiers FIRST COUNT [DIAGNOSTICS]"
                    .into(),
            );
        }
        (
            frontier_cases(args[3].parse()?, args[4].parse()?)?,
            args.get(5),
        )
    } else if args.get(2).is_some_and(|s| s == "--representatives") {
        if args.len() != 6 && args.len() != 7 {
            return Err("usage: protocol-shapes REPOSITORY OUTPUT --representatives DEPTH FIRST COUNT [DIAGNOSTICS]".into());
        }
        (
            representative_cases(args[3].parse()?, args[4].parse()?, args[5].parse()?)?,
            args.get(6),
        )
    } else if args
        .get(2)
        .is_some_and(|s| s == "--interactions" || s == "--spines")
    {
        if args.len() != 6 && args.len() != 7 {
            return Err("usage: protocol-shapes REPOSITORY OUTPUT --interactions LENGTH FIRST COUNT [DIAGNOSTICS]".into());
        }
        (
            interaction_cases(
                args[3].parse()?,
                args[4].parse()?,
                args[5].parse()?,
                args[2] == "--spines",
            )?,
            args.get(6),
        )
    } else if args.get(2).is_some_and(|s| s == "--syntax-depth") {
        if args.len() != 6 && args.len() != 7 {
            return Err("usage: protocol-shapes REPOSITORY OUTPUT --syntax-depth DEPTH FIRST COUNT [DIAGNOSTICS]".into());
        }
        (
            syntax_cases(args[3].parse()?, args[4].parse()?, args[5].parse()?)?,
            args.get(6),
        )
    } else {
        if args.len() != 3 && args.len() != 5 && args.len() != 6 {
            return Err("usage: protocol-shapes REPOSITORY OUTPUT OPERATOR_BUDGET [SHARD SHARDS [DIAGNOSTICS]]".into());
        }
        let budget = args[2].parse()?;
        let (shard, shards) = if args.len() >= 5 {
            (args[3].parse()?, args[4].parse()?)
        } else {
            (0, 1)
        };
        if budget > 5 || shards == 0 || shard >= shards {
            return Err("invalid operator-budget catalog or shard".into());
        }
        (template_cases(budget, shard, shards), args.get(5))
    };
    emit_cases(
        PathBuf::from(&args[0]).canonicalize()?,
        PathBuf::from(&args[1]),
        catalog,
        diagnostics,
    )
}

fn syntax_matrix(
    depth: usize,
    first: num_bigint::BigUint,
    count: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    if depth > 8 {
        return Err("syntax depth must be at most eight".into());
    }
    let grammar = syntax::Grammar::new(depth);
    range_matrix(first, count, grammar.count(depth).clone())
}

fn range_matrix(
    first: num_bigint::BigUint,
    count: usize,
    total: num_bigint::BigUint,
) -> Result<String, Box<dyn std::error::Error>> {
    if count == 0 || count.div_ceil(32) > 256 {
        return Err("invalid candidate range: split ranges larger than 256 CI jobs".into());
    }
    if &first + count > total {
        return Err("candidate range exceeds its population".into());
    }
    let entries = (0..count)
        .step_by(32)
        .enumerate()
        .map(|(partition, offset)| {
            format!(
                r#"{{"partition":{partition},"first":"{}","count":{}}}"#,
                &first + offset,
                (count - offset).min(32)
            )
        })
        .collect::<Vec<_>>();
    Ok(format!(r#"{{"include":[{}]}}"#, entries.join(",")))
}

fn context_matrix(
    length: usize,
    first: usize,
    count: usize,
    spine: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let (population, construct) = if spine {
        (
            interactions::spine_count(length),
            interactions::spine as fn(usize, usize) -> interactions::Interaction,
        )
    } else {
        (
            interactions::count(length),
            interactions::protocol as fn(usize, usize) -> interactions::Interaction,
        )
    };
    let end = first.checked_add(count).ok_or("context range overflow")?;
    if count == 0 || end > population {
        return Err("context range exceeds its population".into());
    }
    let mut entries = Vec::new();
    let mut start = first;
    let mut work = 0usize;
    for rank in first..end {
        let bound = plans::parallel_pair_bound(&construct(length, rank).source);
        if rank != start && (rank - start == 32 || work.saturating_add(bound) > 8192) {
            entries.push((start, rank - start));
            start = rank;
            work = 0;
        }
        work = work.checked_add(bound).ok_or("context workload overflow")?;
    }
    entries.push((start, end - start));
    if entries.len() > 256 {
        return Err("context workload exceeds 256 CI jobs; split this candidate interval".into());
    }
    Ok(format!(
        r#"{{"include":[{}]}}"#,
        entries
            .iter()
            .enumerate()
            .map(|(partition, (first, count))| format!(
                r#"{{"partition":{partition},"first":"{first}","count":{count}}}"#
            ))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

fn template_cases(budget: usize, shard: usize, shards: usize) -> CaseSet {
    let all = catalog(budget);
    let counts = binary_counts(budget);
    let binary_total = counts.total(true, Population::Exchange);
    let resource_total = all.len() + binary_total;
    let ordinary_total = counts.total(false, Population::Exchange);
    let delegated_counts = delegated::Counts::new(budget);
    let delegated_total = delegated_counts.total(Population::Exchange);
    let exchange_cases = 4 * resource_total + 2 * (ordinary_total + delegated_total);
    let resource_converging_end = exchange_cases + 4 * counts.total(true, Population::Converging);
    let ordinary_converging_end =
        resource_converging_end + 2 * counts.total(false, Population::Converging);
    let operator_cases =
        ordinary_converging_end + 2 * delegated_counts.total(Population::Converging);
    let frontier_cases = if budget >= 2 { frontiers::COUNT } else { 0 };
    let total_cases = operator_cases + frontier_cases;
    let mut cases = Vec::new();
    let mut delegated_cases = BTreeSet::new();
    let mut expected_rejections = BTreeMap::new();
    let mut negative_sources = BTreeMap::new();
    let mut max_label = 0;
    let mut max_resolvers = 0;
    // Unrank only the cases assigned to this shard. The exchange-only prefix
    // retains its IDs; mixed-leaf populations add no duplicate trees to it.
    for case in (shard..total_cases).step_by(shards) {
        if case >= operator_cases {
            let rank = case - operator_cases;
            let (mut source, identity) = frontiers::protocol(rank);
            if let Some(unpermitted) = frontiers::unpermitted_reentry(rank) {
                negative_sources.insert(format!("frontier_reentry_{rank}"), (
                    unpermitted,
                    "rolled reentry requires permission from the resource's entry owner; help: let the resource's entry owner authorize each visit inside the roll",
                ));
            }
            let (labels, resolvers) = syntax::number(&mut source);
            if context::has_transfer(&source) {
                delegated_cases.insert(case);
            }
            max_label = max_label.max(labels);
            max_resolvers = max_resolvers.max(resolvers);
            cases.push((case, identity, 2, 0, source, resolvers));
            continue;
        }
        let resource_variant =
            |variant: usize| (if variant.is_multiple_of(2) { 2 } else { 0 }, variant >= 2);
        let (word, service, swap) = if case < 4 * resource_total {
            let index = case / 4;
            let shape = all.get(index).cloned().unwrap_or_else(|| {
                CaseShape::Binary(counts.shape(index - all.len(), true, Population::Exchange))
            });
            let (service, swap) = resource_variant(case % 4);
            (shape, service, swap)
        } else if case < exchange_cases {
            let index = (case - 4 * resource_total) / 2;
            let shape = if index < ordinary_total {
                CaseShape::OrdinaryBinary(counts.shape(index, false, Population::Exchange))
            } else {
                CaseShape::Delegated(
                    delegated_counts.shape(index - ordinary_total, Population::Exchange),
                )
            };
            (shape, 2, (case - 4 * resource_total) % 2 == 1)
        } else if case < resource_converging_end {
            let offset = case - exchange_cases;
            let (service, swap) = resource_variant(offset % 4);
            (
                CaseShape::Binary(counts.shape(offset / 4, true, Population::Converging)),
                service,
                swap,
            )
        } else if case < ordinary_converging_end {
            let offset = case - resource_converging_end;
            (
                CaseShape::OrdinaryBinary(counts.shape(offset / 2, false, Population::Converging)),
                2,
                offset % 2 == 1,
            )
        } else {
            let offset = case - ordinary_converging_end;
            (
                CaseShape::Delegated(delegated_counts.shape(offset / 2, Population::Converging)),
                2,
                offset % 2 == 1,
            )
        };
        let mut count = 0;
        let mut resolvers = 0;
        let mut global = word.build(&mut count, &mut resolvers, service);
        if swap {
            global.swap_participants();
        }
        if matches!(&word, CaseShape::Delegated(_)) {
            delegated_cases.insert(case);
            let mut bad_label = count;
            negative_sources.insert(format!("parallel_{case}"),(
                    Protocol::Par(Box::new(global.clone()),Box::new(message(&mut bad_label,3,2,false))),
                    "parallel endpoint delegation conflicts with another use; role=3; help: keep the transfer and dependent resource operations in sequence"
                ));
            if let CaseShape::Delegated(body) = &word
                && body.has_uninformed_reentry(1, 0)
            {
                expected_rejections.insert(case, "ReentryResourceKnowledge");
                negative_sources.insert(
                        format!("reentry_{case}"),
                        (
                            global.clone(),
                            "rolled reentry requires permission from the resource's entry owner; help: let the resource's entry owner authorize each visit inside the roll",
                        ),
                    );
            }
        }
        // Role 2 never holds the resource. A collector present in only one
        // choice arm has no notification when the other arm is selected.
        // Derive this violation from source, before consulting diagnostics.
        if !expected_rejections.contains_key(&case) && global.has_one_sided_choice(2) {
            expected_rejections.insert(case, "MissingBranchKnowledge");
            negative_sources.insert(
                format!("knowledge_{case}"),
                (global.clone(), "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation; role=2; help: notify the affected role of the branch, or make its local paths mergeable; keep terminal acknowledgments within the selected arm"),
            );
        }
        max_label = max_label.max(u16::from(count));
        max_resolvers = max_resolvers.max(resolvers);
        cases.push((
            case,
            format!("{word:?}"),
            service,
            u8::from(swap),
            global,
            resolvers,
        ));
    }
    CaseSet {
        namespace: format!("operators-{budget}-{shard}-of-{shards}"),
        description: format!(
            "operator_budget={budget} parallel_roll_continuations={frontier_cases} shard={shard}/{shards}"
        ),
        total: total_cases.to_string(),
        cases,
        delegated: delegated_cases,
        expected_rejections,
        negative_sources,
        max_label,
        max_resolvers,
        origin: CatalogOrigin::Templates,
    }
}

fn frontier_cases(first: usize, count: usize) -> Result<CaseSet, Box<dyn std::error::Error>> {
    if count == 0
        || first
            .checked_add(count)
            .is_none_or(|end| end > frontiers::COUNT)
    {
        return Err("frontier range exceeds the parallel-roll continuation population".into());
    }
    let mut catalog = CaseSet {
        namespace: format!("frontiers-{first}-count-{count}"),
        description: format!("parallel_roll_continuations first={first} count={count}"),
        total: frontiers::COUNT.to_string(),
        cases: Vec::new(),
        delegated: BTreeSet::new(),
        expected_rejections: BTreeMap::new(),
        negative_sources: BTreeMap::new(),
        max_label: 0,
        max_resolvers: 0,
        origin: CatalogOrigin::Contexts,
    };
    for rank in first..first + count {
        let (mut source, identity) = frontiers::protocol(rank);
        if let Some(unpermitted) = frontiers::unpermitted_reentry(rank) {
            catalog.negative_sources.insert(format!("frontier_reentry_{rank}"), (
                unpermitted,
                "rolled reentry requires permission from the resource's entry owner; help: let the resource's entry owner authorize each visit inside the roll",
            ));
        }
        let (labels, resolvers) = syntax::number(&mut source);
        let case = catalog.cases.len();
        if context::has_transfer(&source) {
            catalog.delegated.insert(case);
        }
        catalog.max_label = catalog.max_label.max(labels);
        catalog.max_resolvers = catalog.max_resolvers.max(resolvers);
        catalog
            .cases
            .push((case, identity, 2, 0, source, resolvers));
    }
    Ok(catalog)
}

fn syntax_cases(
    depth: usize,
    first: num_bigint::BigUint,
    count: usize,
) -> Result<CaseSet, Box<dyn std::error::Error>> {
    if depth > 8 || count == 0 {
        return Err("syntax depth must be at most eight and count must be positive".into());
    }
    let grammar = syntax::Grammar::new(depth);
    if &first + count > *grammar.count(depth) {
        return Err("syntax range exceeds its exact-height population".into());
    }
    let mut catalog = CaseSet {
        namespace: format!("height-{depth}-count-{count}"),
        description: format!("syntax_depth={depth} first={first} count={count}"),
        total: grammar.count(depth).to_string(),
        cases: Vec::new(),
        delegated: BTreeSet::new(),
        expected_rejections: BTreeMap::new(),
        negative_sources: BTreeMap::new(),
        max_label: 0,
        max_resolvers: 0,
        origin: CatalogOrigin::Syntax,
    };
    for case in 0..count {
        let rank = &first + case;
        let (global, labels, resolvers) = grammar.protocol(depth, rank.clone());
        let mut messages = Vec::new();
        global.messages(&mut messages);
        let owner = messages
            .iter()
            .find_map(|message| match message {
                Protocol::Message {
                    from, owned: true, ..
                } => Some(*from),
                _ => None,
            })
            .unwrap_or(3);
        if localside::mobile(&global) {
            catalog.delegated.insert(case);
        }
        // These source rules do not depend on projection's verdict. Unknown
        // rejections stay unresolved instead of becoming expected failures.
        if let Some(&(problem, reason)) = syntax_violations(&global).first() {
            catalog.expected_rejections.insert(case, problem);
            catalog
                .negative_sources
                .insert(format!("syntax_{case}"), (global.clone(), reason));
        }
        catalog.max_label = catalog.max_label.max(labels);
        catalog.max_resolvers = catalog.max_resolvers.max(resolvers);
        catalog.cases.push((
            case,
            format!("height={depth}/rank={rank}"),
            2,
            owner,
            global,
            resolvers,
        ));
    }
    Ok(catalog)
}

fn syntax_violations(source: &Protocol) -> Vec<(&'static str, &'static str)> {
    let ownership = ownership::violations(source);
    if ownership.is_empty() {
        choice::violations(source)
    } else {
        ownership.into_iter().collect()
    }
}

fn representative_cases(
    depth: usize,
    first: usize,
    count: usize,
) -> Result<CaseSet, Box<dyn std::error::Error>> {
    let ranks = representatives::catalog(depth)?;
    if count == 0 || first.checked_add(count).is_none_or(|end| end > ranks.len()) {
        return Err("source representative range exceeds its population".into());
    }
    let grammar = syntax::Grammar::new(depth);
    let mut catalog = CaseSet {
        namespace: format!("representatives-{depth}-{first}-count-{count}"),
        description: format!("representative_depth={depth} first={first} count={count}"),
        total: ranks.len().to_string(),
        cases: Vec::new(),
        delegated: BTreeSet::new(),
        expected_rejections: BTreeMap::new(),
        negative_sources: BTreeMap::new(),
        max_label: 0,
        max_resolvers: 0,
        origin: CatalogOrigin::Representatives,
    };
    for (case, &rank) in ranks[first..first + count].iter().enumerate() {
        let (source, labels, resolvers) = grammar.protocol(depth, rank.into());
        assert!(context::valid(&source));
        assert_eq!(source, symmetry::canonical(&source));
        let mut messages = Vec::new();
        source.messages(&mut messages);
        let owner = messages
            .iter()
            .find_map(|message| match message {
                Protocol::Message {
                    from, owned: true, ..
                } => Some(*from),
                _ => None,
            })
            .unwrap_or(3);
        if localside::mobile(&source) {
            catalog.delegated.insert(case);
        }
        catalog.max_label = catalog.max_label.max(labels);
        catalog.max_resolvers = catalog.max_resolvers.max(resolvers);
        catalog.cases.push((
            case,
            format!(
                "source-depth={depth}/ordinal={}/raw-rank={rank}",
                first + case
            ),
            2,
            owner,
            source,
            resolvers,
        ));
    }
    Ok(catalog)
}

fn matches_source_violation(verdict: &str, problem: &str, reason: &str) -> bool {
    let role = reason
        .split("; ")
        .find_map(|field| field.strip_prefix("role="));
    let role_matches = match role {
        Some(role) => verdict.contains(&format!("role: Some({role}),")),
        None => verdict.contains("role: None,"),
    };
    role_matches
        && verdict.starts_with(&format!(
            "rejected:ProjectionDiagnostic {{ problem: {problem},"
        ))
}

fn interaction_cases(
    length: usize,
    first: usize,
    count: usize,
    spine: bool,
) -> Result<CaseSet, Box<dyn std::error::Error>> {
    if length > 5 {
        return Err("interaction length must be at most five".into());
    }
    let (family, population, construct) = if spine {
        (
            "spines",
            interactions::spine_count(length),
            interactions::spine as fn(usize, usize) -> interactions::Interaction,
        )
    } else {
        (
            "interactions",
            interactions::count(length),
            interactions::protocol as fn(usize, usize) -> interactions::Interaction,
        )
    };
    if count == 0 || first.checked_add(count).is_none_or(|end| end > population) {
        return Err("interaction range exceeds the length-zero-through-five population".into());
    }
    let mut catalog = CaseSet {
        namespace: format!("{family}-{length}-{first}-count-{count}"),
        description: format!("{family}_length={length} first={first} count={count}"),
        total: population.to_string(),
        cases: Vec::new(),
        delegated: BTreeSet::new(),
        expected_rejections: BTreeMap::new(),
        negative_sources: BTreeMap::new(),
        max_label: 0,
        max_resolvers: 0,
        origin: CatalogOrigin::Contexts,
    };
    let mut representatives = BTreeSet::new();
    for rank in first..first + count {
        let interactions::Interaction {
            source,
            labels,
            resolvers,
            identity,
            unpermitted_reentry,
        } = construct(length, rank);
        if let Some(unpermitted) = unpermitted_reentry {
            catalog.negative_sources.insert(format!("reentry_{rank}"), (
                symmetry::canonical(&unpermitted),
                "rolled reentry requires permission from the resource's entry owner; help: let the resource's entry owner authorize each visit inside the roll",
            ));
        }
        let source = symmetry::canonical(&source);
        if !representatives.insert(source.clone()) {
            return Err(format!("duplicate interaction source at rank {rank}").into());
        }
        let case = catalog.cases.len();
        catalog.delegated.insert(case);
        catalog.max_label = catalog.max_label.max(labels);
        catalog.max_resolvers = catalog.max_resolvers.max(resolvers);
        catalog.cases.push((
            case,
            format!(
                "{}={length}/rank={rank}/{identity}",
                if spine { "spine" } else { "interaction" }
            ),
            2,
            0,
            source,
            resolvers,
        ));
    }
    Ok(catalog)
}

fn contract_collision_cases(
    first: usize,
    count: usize,
) -> Result<CaseSet, Box<dyn std::error::Error>> {
    if count == 0
        || first
            .checked_add(count)
            .is_none_or(|end| end > contracts::COUNT)
    {
        return Err("contract collision range exceeds its population".into());
    }
    let mut cases = Vec::new();
    let mut delegated = BTreeSet::new();
    let mut max_label = 0;
    let mut max_resolvers = 0;
    for rank in first..first + count {
        let mut global = contracts::protocol(rank);
        let (labels, resolvers) = syntax::number(&mut global);
        max_label = max_label.max(labels);
        max_resolvers = max_resolvers.max(resolvers);
        if localside::mobile(&global) {
            delegated.insert(rank);
        }
        cases.push((
            rank,
            format!("contract-collision={rank}"),
            2,
            0,
            global,
            resolvers,
        ));
    }
    Ok(CaseSet {
        namespace: format!("contracts-{first}-{count}"),
        description: format!("message_contract_collisions first={first} count={count}"),
        total: contracts::COUNT.to_string(),
        cases,
        delegated,
        expected_rejections: BTreeMap::new(),
        negative_sources: BTreeMap::new(),
        max_label,
        max_resolvers,
        origin: CatalogOrigin::Contexts,
    })
}

fn contract_cases(mut catalog: CaseSet) -> CaseSet {
    let mut cases = Vec::new();
    let mut delegated = BTreeSet::new();
    let mut rejections = BTreeMap::new();
    for (base, identity, service, owner, global, resolvers) in &catalog.cases {
        let reasons = catalog
            .negative_sources
            .values()
            .filter(|(source, _)| source == global)
            .map(|(_, reason)| *reason)
            .collect::<BTreeSet<_>>();
        for (variant, (name, source)) in contracts::variants(global).into_iter().enumerate() {
            let case = base
                .checked_mul(3)
                .and_then(|id| id.checked_add(variant))
                .expect("contract case identity overflow");
            if catalog.delegated.contains(base) {
                delegated.insert(case);
            }
            if let Some(problem) = catalog.expected_rejections.get(base) {
                rejections.insert(case, *problem);
                for (index, reason) in reasons.iter().enumerate() {
                    catalog.negative_sources.insert(
                        format!("contracts_{case}_{index}"),
                        (source.clone(), *reason),
                    );
                }
            }
            cases.push((
                case,
                format!("{identity}/contracts={name}"),
                *service,
                *owner,
                source,
                *resolvers,
            ));
        }
    }
    catalog.cases = cases;
    catalog.delegated = delegated;
    catalog.expected_rejections = rejections;
    catalog
}

fn emit_cases(
    root: PathBuf,
    output: PathBuf,
    catalog: CaseSet,
    diagnostics: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = contract_cases(catalog);
    let CaseSet {
        namespace,
        description,
        total: total_cases,
        cases,
        delegated: delegated_cases,
        expected_rejections,
        mut negative_sources,
        max_label,
        max_resolvers,
        origin,
    } = catalog;
    let parallel_pairs = matches!(origin, CatalogOrigin::Contexts);
    if cases.is_empty() {
        return Err("empty shard".into());
    }
    fs::create_dir_all(output.join("src"))?;
    fs::write(
        output.join("catalog-scope.txt"),
        format!(
            "{description}\nexact_syntax_population={total_cases}\nmessage_contract_assignments=distinct,reuse-first,reuse-last\ncontract_assignment_scope=choice_entries_distinct;identical_assignments_deduplicated\nemitted_candidates={}\nnamespace={namespace}\n",
            cases.len()
        ),
    )?;
    let mut identities = String::from("case\tidentity\tsyntax_depth\toperator_count\n");
    for (case, identity, _, _, global, _) in &cases {
        writeln!(
            identities,
            "{case}\t{identity}\t{}\t{}",
            global.syntax_depth(),
            global.operator_count()
        )?;
    }
    fs::write(output.join("case-identities.tsv"), identities)?;
    let mut assignments = String::from("case\tevent\tmessage_label\towned\n");
    for (case, _, _, _, source, _) in &cases {
        let mut messages = Vec::new();
        source.messages(&mut messages);
        for message in messages {
            let Protocol::Message {
                event,
                contract,
                owned,
                ..
            } = message
            else {
                unreachable!()
            };
            writeln!(assignments, "{case}\t{event}\t{contract}\t{owned}")?;
        }
    }
    fs::write(output.join("contract-assignments.tsv"), assignments)?;
    let mut candidate_inputs = Vec::new();
    for (case, _, _, _, global, resolvers) in &cases {
        let mut nodes = Vec::new();
        global.json_nodes(&mut nodes);
        candidate_inputs.push(format!(
            r#"{{"name":{case},"nodes":[{}],"resolvers":{resolvers},"parallelCoverage":{parallel_pairs}}}"#,
            nodes.join(",")
        ));
    }
    fs::write(
        output.join("catalog.json"),
        format!("[{}]\n", candidate_inputs.join(",")),
    )?;
    let trace_directory = output.join("traces");
    fs::create_dir_all(&trace_directory)?;
    fs::write(
        output.join("Cargo.toml"),
        format!(
            r#"[package]
name = "hibana-generated-protocol-shapes-{namespace}"
version = "0.0.0"
edition = "2024"
publish = false
autolib = false
[workspace]
[dependencies]
hibana = {{ path = {root:?} }}
tokio = {{ version = "=1.48.0", features = ["macros", "rt", "time", "test-util"] }}
[profile.dev]
opt-level = 1
debug = 0
debug-assertions = true
overflow-checks = true
incremental = false
[profile.dev.package.hibana]
opt-level = 2
[lints.rust]
long_running_const_eval = "forbid"
"#
        ),
    )?;
    let mut diagnostic_source =
        String::from("#![forbid(long_running_const_eval)]\nuse hibana::g;\n");
    if cases.iter().any(|(_, _, _, _, global, _)| {
        let mut messages = Vec::new();
        global.messages(&mut messages);
        messages
            .iter()
            .any(|m| matches!(m, Protocol::Message { owned: true, .. }))
    }) {
        diagnostic_source.push_str("use hibana::Endpoint;\n");
    }
    diagnostic_source.push_str("fn main(){\n");
    for (case, ..) in &cases {
        writeln!(diagnostic_source, "shape_{case}();")?;
        // Each case has its own type aliases, just as in the endpoint driver.
    }
    diagnostic_source.push_str("}\n");
    for (case, _, _, _, global, _) in &cases {
        writeln!(diagnostic_source, "fn shape_{case}(){{")?;
        // g::send rejects this payload during codegen, before diagnose can
        // run. The separate rejected/ build must establish this source rule.
        if expected_rejections.get(case) == Some(&"NonUnitSelfMessage") {
            writeln!(
                diagnostic_source,
                "println!(\"{case}\\tsource-rejected:NonUnitSelfMessage\");}}"
            )?;
            continue;
        }
        let mut messages = Vec::new();
        global.messages(&mut messages);
        for m in messages {
            let Protocol::Message {
                event: label,
                contract,
                owned,
                ..
            } = m
            else {
                unreachable!()
            };
            let payload = if *owned { "Endpoint<'r,3>" } else { "()" };
            writeln!(
                diagnostic_source,
                "type M{label}<'r>=g::Msg<{contract},{payload}>;"
            )?;
        }
        writeln!(
            diagnostic_source,
            "let global={};match g::diagnose(&global){{None=>println!(\"{case}\\taccepted\"),Some(reason)=>println!(\"{case}\\trejected:{{reason:?}}\")}}\n}}",
            global.rust()
        )?;
    }
    fs::create_dir_all(output.join("src/bin"))?;
    write_if_changed(&output.join("src/bin/diagnose.rs"), &diagnostic_source)?;
    if diagnostics.is_none() {
        println!(
            "syntax catalog candidates={} {description} total_shapes={total_cases}; awaiting diagnostics before localside emission",
            cases.len()
        );
        return Ok(());
    }
    let classifications = if let Some(path) = diagnostics {
        let mut found = BTreeMap::new();
        for line in fs::read_to_string(path)?.lines() {
            let (case, verdict) = line.split_once('\t').ok_or("invalid diagnostic record")?;
            let case = case.parse::<usize>()?;
            let constructor_rejected = verdict == "source-rejected:NonUnitSelfMessage"
                && expected_rejections.get(&case) == Some(&"NonUnitSelfMessage");
            if verdict != "accepted"
                && !constructor_rejected
                && !verdict
                    .strip_prefix("rejected:")
                    .is_some_and(|reason| !reason.is_empty())
            {
                return Err(format!("invalid diagnostic verdict for shape {case}").into());
            }
            if let Some(expected) = expected_rejections.get(&case)
                && !constructor_rejected
            {
                if origin.requires_diagnostics() {
                    let global = &cases
                        .iter()
                        .find(|c| c.0 == case)
                        .ok_or("unknown syntax diagnostic")?
                        .4;
                    let proven = syntax_violations(global);
                    // Several source violations can coexist. Select only a
                    // proven violation, then require this exact compile error.
                    let reason = proven.iter().find_map(|(problem, reason)| {
                        matches_source_violation(verdict, problem, reason).then_some(*reason)
                    }).ok_or_else(|| format!("known illegal shape {case} did not report a source-proven violation {proven:?}: {verdict}"))?;
                    negative_sources.insert(format!("syntax_{case}"), (global.clone(), reason));
                } else if !verdict.starts_with(&format!(
                    "rejected:ProjectionDiagnostic {{ problem: {expected},"
                )) {
                    return Err(format!(
                        "known illegal shape {case} did not report {expected}: {verdict}"
                    )
                    .into());
                }
            }
            if found.insert(case, verdict == "accepted").is_some() {
                return Err(format!("duplicate diagnostic for shape {case}").into());
            }
        }
        if found.keys().copied().collect::<BTreeSet<_>>() != cases.iter().map(|c| c.0).collect() {
            return Err("missing or unknown shape diagnostic".into());
        }
        Some(found)
    } else {
        None
    };
    let negative_root = output.join("rejected");
    if negative_root.exists() {
        fs::remove_dir_all(&negative_root)?;
    }
    fs::create_dir_all(negative_root.join("src/bin"))?;
    fs::write(
        negative_root.join("Cargo.toml"),
        format!(
            "[package]\nname=\"hibana-rejected-shapes-{namespace}\"\nversion=\"0.0.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\nhibana={{path={root:?}}}\n[lints.rust]\nlong_running_const_eval=\"forbid\"\n"
        ),
    )?;
    let mut negative_inventory = String::new();
    for (name, (global, reason)) in &negative_sources {
        let mut source = String::from(
            "#![forbid(long_running_const_eval)]\nuse hibana::{Endpoint,g,runtime::program::project};\nfn main(){\n",
        );
        let mut messages = Vec::new();
        global.messages(&mut messages);
        for message in messages {
            let Protocol::Message {
                event: label,
                contract,
                owned,
                ..
            } = message
            else {
                unreachable!()
            };
            writeln!(
                source,
                "type M{label}<'r>=g::Msg<{contract},{}>;",
                if *owned { "Endpoint<'r,3>" } else { "()" }
            )?;
        }
        if *reason == "self-role actions require the unit payload" {
            writeln!(source, "let _global={};}}", global.rust())?;
        } else {
            writeln!(
                source,
                "let global={};let _=project::<0>(&global);}}",
                global.rust()
            )?;
        }
        fs::write(
            negative_root.join("src/bin").join(format!("{name}.rs")),
            source,
        )?;
        writeln!(negative_inventory, "{name}\t{reason}")?;
    }
    fs::write(negative_root.join("inventory.tsv"), negative_inventory)?;
    let mut source = format!(
        "#![cfg(test)]\n#![forbid(long_running_const_eval)]\n#[path={:?}]\nmod in_memory;\n#[path={:?}]\nmod planned_json;\n#[path={:?}]\nmod probe_data;\n",
        root.join("examples/support/in_memory.rs"),
        root.join(".github/ci-tests/kani_json.rs"),
        root.join(".github/ci-tests/protocol_shape_probe_data.rs")
    );
    let registers = (0..max_resolvers.max(1))
        .map(|id| format!("register!({id});\n"))
        .collect::<String>();
    let dispatch = (0..max_label)
        .map(|label| format!("{label} => execute!({label}),\n"))
        .collect::<String>();
    let mut driver = fs::read_to_string(root.join(".github/ci-tests/protocol_shape_driver.rs"))?;
    driver.push_str(&fs::read_to_string(
        root.join(".github/ci-tests/protocol_shape_observations.rs"),
    )?);
    // This source is part of the generated test module, not a nested module.
    source.push_str(
        &driver
            .replacen(
                "//! Shared replay engine for generated tests; contains no application protocol.",
                "// Shared replay engine for generated tests.",
                1,
            )
            .replace("register_resolvers!();", &registers)
            .replace("0 => dispatch_labels!(),", &dispatch),
    );
    let (mut probes, mut trace_count, mut operations, mut concurrent_traces) = (0, 0, 0, 0);
    let (mut continuation_shapes, mut trace_prefix_probes) = (0, 0);
    let mut delegated_concurrent_runs = 0;
    let mut scoped_executions = 0;
    let mut localsides = Vec::new();
    let mut inputs = Vec::new();
    let mut catalog_inputs = Vec::new();
    let mut scope_inputs = Vec::new();
    let scope_directory = output.join("scope-plans");
    fs::create_dir_all(&scope_directory)?;
    let mut scope_paths = BTreeSet::new();
    let script_directory = output.join("scripts");
    fs::create_dir_all(&script_directory)?;
    let mut script_paths = BTreeSet::new();
    let mut trace_paths = BTreeSet::new();
    let mut local_rejection_probes = 0usize;
    let mut rejected = 0;
    let mut source_coverage = coverage::Coverage::default();
    let mut case_depths = String::from("case\tsyntax_depth\toperator_count\n");
    let mut catalog_max_depth = 0;
    let mut emitted_max_depth = 0;
    for (case, word, service, owner, global, resolvers) in &cases {
        let syntax_depth = global.syntax_depth();
        catalog_max_depth = catalog_max_depth.max(syntax_depth);
        writeln!(
            case_depths,
            "{case}\t{syntax_depth}\t{}",
            global.operator_count()
        )?;
        let mut nodes = Vec::new();
        global.json_nodes(&mut nodes);
        let input = format!(
            r#"{{"name":{case},"nodes":[{}],"resolvers":{resolvers},"parallelCoverage":{parallel_pairs}}}"#,
            nodes.join(",")
        );
        catalog_inputs.push(input.clone());
        if expected_rejections.contains_key(case) {
            continue;
        }
        if classifications
            .as_ref()
            .is_some_and(|verdicts| !verdicts[case])
        {
            rejected += 1;
            continue;
        }
        emitted_max_depth = emitted_max_depth.max(syntax_depth);
        source_coverage.add(*case, global);
        let mut messages = Vec::new();
        global.messages(&mut messages);
        let plans = plans::Plans::new(
            global,
            if parallel_pairs {
                plans::Coverage::ParallelPairs
            } else {
                plans::Coverage::Exhaustive
            },
        );
        let traces = plans
            .executions()
            .map(|execution| execution.trace())
            .collect::<BTreeSet<_>>();
        let mut decisions = BTreeMap::new();
        global.resolver_choices(&mut Vec::new(), &mut decisions);
        let patterns = decisions::patterns(global, *resolvers);
        if global.prefix_visits() != 0 {
            continuation_shapes += 1;
            let mut prefixes = BTreeSet::new();
            for trace in &traces {
                let mut prefix = Vec::new();
                for &(label, _) in &trace.messages {
                    for receive in [false, true] {
                        if messages.iter().any(|message| matches!(message, Protocol::Message { from, to, event: candidate, .. } if *candidate == label && from == to)) && receive {
                            continue;
                        }
                        prefix.push(2 * u16::from(label) + u16::from(receive));
                        prefixes.insert(prefix.clone());
                    }
                }
            }
            trace_prefix_probes += prefixes.len() * patterns.len() * messages.len() * 2;
        }
        inputs.push(input);
        let start = source.len();
        writeln!(
            source,
            "#[tokio::test(flavor=\"current_thread\",start_paused=true)]\nasync fn shape_{case}() {{"
        )?;
        for m in &messages {
            let Protocol::Message {
                event: label,
                contract,
                owned,
                ..
            } = m
            else {
                unreachable!()
            };
            let payload = if *owned { "Endpoint<'r,3>" } else { "()" };
            writeln!(source, "type M{label}<'r>=g::Msg<{contract},{payload}>;")?;
        }
        writeln!(
            source,
            "let global={};\nlet programs=Programs{{p0:{},p1:{},p2:{},p3:{},resource:{},owner:{owner},resolver_roles:&{:?}}};",
            global.rust(),
            if localside::present(global, 0, None) {
                "Some(project::<0>(&global))"
            } else {
                "None"
            },
            if localside::present(global, 1, None) {
                "Some(project::<1>(&global))"
            } else {
                "None"
            },
            if localside::present(global, 2, None) {
                "Some(project::<2>(&global))"
            } else {
                "None"
            },
            if messages.iter().any(|m| matches!(
                m,
                Protocol::Message { owned: true, .. }
                    | Protocol::Message { from: 3, .. }
                    | Protocol::Message { to: 3, .. }
            )) {
                "Some(project::<3>(&global))"
            } else {
                "None"
            },
            localside::mobile(global),
            {
                let mut roles = Vec::new();
                global.resolver_roles(&mut roles);
                roles
            }
        )?;
        let local = localside::runs(&plans, *owner, *case);
        source.push_str(&local);
        let script_path = script_directory.join(format!("{case}.jsonl"));
        plans.write_scripts(std::io::BufWriter::new(fs::File::create(&script_path)?))?;
        script_paths.insert(script_path);
        let scope_path = scope_directory.join(format!("{case}.jsonl"));
        let (script_count, scope_count) = scope_probes::write_inventory(
            &plans,
            *owner,
            *case,
            std::io::BufWriter::new(fs::File::create(&scope_path)?),
        )?;
        scope_paths.insert(scope_path);
        scope_inputs.push(format!(
            r#"{{"name":{case},"scripts":{script_count},"probes":{scope_count}}}"#
        ));
        local_rejection_probes += scope_count;
        scoped_executions += script_count;
        if delegated_cases.contains(case) {
            delegated_concurrent_runs += script_count;
        }
        writeln!(
            source,
            "qualify(programs,Scenario{{resolver_count:{resolvers},id:{case},name:\"{word}/service={service}/owner={owner}\",messages:&["
        )?;
        for m in &messages {
            let Protocol::Message {
                from,
                to,
                event: label,
                contract,
                owned,
                ..
            } = m
            else {
                unreachable!()
            };
            writeln!(
                source,
                "Message{{from:{from},to:{to},event:{label},contract:{contract},owned:{owned},decisions:&{:?}}},",
                decisions[label]
            )?;
        }
        source.push_str("],choices:&[\n");
        for choice in &patterns {
            writeln!(source, "&{choice:?},")?;
        }
        let mut trace_data = String::new();
        for trace in &traces {
            trace_data.push_str(&trace.line());
            operations += trace.messages.iter().map(|(label,_)| if messages.iter().any(|m|matches!(m,Protocol::Message{from,to,event:candidate,..} if candidate==label && from==to)) {1} else {2}).sum::<usize>();
        }
        let trace_path = trace_directory.join(format!("{case}.txt"));
        write_if_changed(&trace_path, &trace_data)?;
        trace_paths.insert(trace_path);
        writeln!(
            source,
            "],traces:include_str!(\"../traces/{case}.txt\"),expected_traces:{},prefix_visits:{},completed_plan_words:&completed_plan_words}}).await;\n}}",
            traces.len(),
            global.prefix_visits(),
        )?;
        localsides.push(source.split_off(start));
        probes += messages.len() * 2 * patterns.len();
        trace_count += traces.len();
        if matches!(global, Protocol::Par(..)) {
            concurrent_traces += traces.len();
        }
    }
    // Separate rustc invocations keep generated async types within the local
    // memory guard. Every case remains in the same Cargo test invocation.
    let mut manifest = fs::read_to_string(output.join("Cargo.toml"))?;
    let mut paths = BTreeSet::new();
    for (unit, cases) in localsides.chunks(origin.compilation_width()).enumerate() {
        let name = format!("shapes_{unit}");
        let path = if unit == 0 {
            "src/lib.rs".to_owned()
        } else {
            format!("src/{name}.rs")
        };
        let destination = output.join(&path);
        write_if_changed(&destination, &format!("{source}{}", cases.concat()))?;
        paths.insert(destination);
        writeln!(manifest, "\n[[test]]\nname={name:?}\npath={path:?}")?;
    }
    for entry in fs::read_dir(output.join("src"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("shapes_")
            && !paths.contains(&entry.path())
        {
            fs::remove_file(entry.path())?;
        }
    }
    fs::write(output.join("Cargo.toml"), manifest)?;
    for entry in fs::read_dir(scope_directory)? {
        let path = entry?.path();
        if !scope_paths.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    for entry in fs::read_dir(script_directory)? {
        let path = entry?.path();
        if !script_paths.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    for entry in fs::read_dir(trace_directory)? {
        let path = entry?.path();
        if !trace_paths.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    fs::write(
        output.join("inputs.json"),
        format!("[{}]\n", inputs.join(",")),
    )?;
    fs::write(
        output.join("scope-inputs.json"),
        format!("[{}]\n", scope_inputs.join(",")),
    )?;
    fs::write(
        output.join("catalog.json"),
        format!("[{}]\n", catalog_inputs.join(",")),
    )?;
    fs::write(
        output.join("unclassified-rejections.txt"),
        format!("{rejected}\n"),
    )?;
    fs::write(output.join("case-depths.tsv"), case_depths)?;
    fs::write(
        output.join("source-coverage.tsv"),
        source_coverage.witnesses(),
    )?;
    fs::write(
        output.join("source-coverage-counts.tsv"),
        source_coverage.counts(),
    )?;
    println!(
        "generated shapes={} initial_probes={probes} continuation_shapes={continuation_shapes} trace_prefix_probes={trace_prefix_probes} finite_short_prefix_depth=4 complete_traces={trace_count} trace_operations={operations} concurrent_traces={concurrent_traces} delegated_concurrent_runs={delegated_concurrent_runs} scoped_executions={scoped_executions} {description} catalog_max_syntax_depth={catalog_max_depth} emitted_max_syntax_depth={emitted_max_depth} catalog_shapes={} unclassified_rejections={rejected} total_shapes={}",
        inputs.len(),
        cases.len(),
        total_cases
    );
    println!(
        "planned local rejection probes={local_rejection_probes} parallel_schedule_coverage={}",
        if parallel_pairs {
            "pairs-per-source-path"
        } else {
            "exhaustive"
        }
    );
    Ok(())
}

#[cfg(test)]
mod population_tests {
    use super::*;

    #[test]
    fn smoke_partitions_preserve_every_source_and_negative_control() {
        let whole = template_cases(2, 0, 1);
        let mut cases = BTreeMap::new();
        let mut delegated = BTreeSet::new();
        let mut rejected = BTreeMap::new();
        let mut negative = BTreeMap::new();
        for shard in 0..32 {
            let part = template_cases(2, shard, 32);
            assert_eq!(part.total, whole.total);
            for case in part.cases {
                let mut messages = Vec::new();
                case.4.messages(&mut messages);
                assert!(messages.iter().all(|message| match message {
                    Protocol::Message { event: label, .. } => u16::from(*label) < part.max_label,
                    _ => unreachable!(),
                }));
                assert!(case.5 <= part.max_resolvers);
                assert!(cases.insert(case.0, case).is_none());
            }
            for case in part.delegated {
                assert!(delegated.insert(case));
            }
            for (case, reason) in part.expected_rejections {
                assert!(rejected.insert(case, reason).is_none());
            }
            for (name, source) in part.negative_sources {
                assert!(negative.insert(name, source).is_none());
            }
        }
        assert!(cases.into_values().eq(whole.cases));
        assert_eq!(delegated, whole.delegated);
        assert_eq!(rejected, whole.expected_rejections);
        assert_eq!(negative, whole.negative_sources);
    }

    #[test]
    fn source_representative_ranges_preserve_raw_grammar_identities() {
        let ranks = representatives::catalog(1).unwrap();
        assert_eq!(ranks.len(), 94);
        let catalog = representative_cases(1, 0, ranks.len()).unwrap();
        for ((case, identity, _, _, source, _), rank) in catalog.cases.iter().zip(ranks) {
            assert_eq!(
                *case,
                identity
                    .split('/')
                    .nth(1)
                    .unwrap()
                    .strip_prefix("ordinal=")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            );
            assert_eq!(syntax::Grammar::new(1).rank(source), rank.into());
        }
        assert!(representative_cases(1, 93, 2).is_err());
        assert!(representative_cases(1, 0, 0).is_err());
        assert!(representative_cases(3, 0, 1).is_err());
    }

    #[test]
    fn choice_diagnostics_select_only_source_proven_rules_and_participants() {
        let source = Protocol::Route(
            Box::new(Protocol::Message {
                from: 2,
                to: 0,
                event: 0,
                owned: false,
                contract: 0,
            }),
            Box::new(Protocol::Message {
                from: 0,
                to: 1,
                event: 1,
                owned: true,
                contract: 1,
            }),
        );
        let proven = syntax_violations(&source);
        for role in 0..4 {
            let verdict = format!(
                "rejected:ProjectionDiagnostic {{ problem: MissingBranchKnowledge, role: Some({role}), scope: Some(0) }}"
            );
            assert_eq!(
                proven
                    .iter()
                    .any(|(problem, reason)| matches_source_violation(&verdict, problem, reason)),
                role < 2
            );
        }
        for verdict in [
            "accepted",
            "rejected:ProjectionDiagnostic { problem: OwnershipCapacity, role: None, scope: None }",
            "rejected:ProjectionDiagnostic { problem: RouteControllerMismatch, role: Some(3), scope: None }",
        ] {
            assert!(
                !proven
                    .iter()
                    .any(|(problem, reason)| matches_source_violation(verdict, problem, reason))
            );
        }
        assert!(proven.iter().any(|(problem, reason)| matches_source_violation("rejected:ProjectionDiagnostic { problem: RouteControllerMismatch, role: None, scope: Some(0) }", problem, reason)));
    }

    #[test]
    fn syntax_ci_ranges_have_no_gaps_overlap_or_machine_rank_limit() {
        assert_eq!(
            syntax_matrix(1, 4095u32.into(), 33).unwrap(),
            r#"{"include":[{"partition":0,"first":"4095","count":32},{"partition":1,"first":"4127","count":1}]}"#
        );
        let first = num_bigint::BigUint::from(1u8) << 130usize;
        assert!(
            syntax_matrix(5, first.clone(), 32)
                .unwrap()
                .contains(&format!(r#""first":"{first}""#))
        );
        assert!(syntax_matrix(1, 4127u32.into(), 2).is_err());
        assert!(syntax_matrix(5, 0u8.into(), 8193).is_err());
        assert!(syntax_matrix(0, 0u8.into(), 0).is_err());
        assert_eq!(
            range_matrix(130usize.into(), 32, interactions::count(2).into()).unwrap(),
            r#"{"include":[{"partition":0,"first":"130","count":32}]}"#
        );
        assert!(range_matrix(131usize.into(), 32, interactions::count(2).into()).is_err());
    }

    #[test]
    fn context_work_assignment_preserves_the_range_and_isolates_heavy_cases() {
        for (length, first, count) in [(0, 0, 2), (4, 0, 4802), (5, 33582, 32)] {
            let matrix: serde_json::Value =
                serde_json::from_str(&context_matrix(length, first, count, true).unwrap()).unwrap();
            let mut next = first;
            let entries = matrix["include"].as_array().unwrap();
            for (partition, entry) in entries.iter().enumerate() {
                assert_eq!(entry["partition"], partition);
                let start = entry["first"].as_str().unwrap().parse::<usize>().unwrap();
                let size = entry["count"].as_u64().unwrap() as usize;
                assert_eq!(start, next);
                assert!((1..=32).contains(&size));
                let work = (start..start + size)
                    .map(|rank| {
                        plans::parallel_pair_bound(&interactions::spine(length, rank).source)
                    })
                    .sum::<usize>();
                assert!(size == 1 || work <= 8192);
                next += size;
            }
            assert_eq!(next, first + count);
        }
        assert!(context_matrix(2, 0, 0, true).is_err());
        assert!(context_matrix(2, 97, 2, true).is_err());
        assert!(context_matrix(5, usize::MAX, 1, true).is_err());
    }

    #[test]
    fn operator_count_and_syntax_depth_are_different_bounds() {
        let mut label = 0;
        let a = message(&mut label, 0, 1, false);
        let b = message(&mut label, 1, 0, false);
        let c = message(&mut label, 0, 2, false);
        let d = message(&mut label, 2, 0, false);
        let balanced = Protocol::Par(
            Box::new(Protocol::Seq(Box::new(a.clone()), Box::new(b.clone()))),
            Box::new(Protocol::Resolved(
                Box::new(c.clone()),
                Box::new(d.clone()),
                0,
            )),
        );
        let sequential = Protocol::Seq(
            Box::new(a),
            Box::new(Protocol::Seq(
                Box::new(b),
                Box::new(Protocol::Seq(Box::new(c), Box::new(d))),
            )),
        );
        assert_eq!(balanced.operator_count(), 3);
        assert_eq!(sequential.operator_count(), 3);
        assert_eq!(balanced.syntax_depth(), 2);
        assert_eq!(sequential.syntax_depth(), 3);
        let repeated = Protocol::Roll(Box::new(balanced));
        assert_eq!(repeated.syntax_depth(), 3);
        assert_eq!(repeated.operator_count(), 4);
    }

    #[test]
    fn a_collector_must_participate_in_both_choice_arms() {
        let mut label = 0;
        let absent = message(&mut label, 0, 1, false);
        let present = message(&mut label, 0, 2, false);
        let route = Protocol::Route(Box::new(absent.clone()), Box::new(present.clone()));
        assert!(route.has_one_sided_choice(2));
        assert!(Protocol::Roll(Box::new(route.clone())).has_one_sided_choice(2));
        assert!(
            Protocol::Seq(Box::new(present.clone()), Box::new(route.clone()))
                .has_one_sided_choice(2)
        );
        assert!(Protocol::Par(Box::new(route), Box::new(present.clone())).has_one_sided_choice(2));
        assert!(
            !Protocol::Route(Box::new(absent.clone()), Box::new(absent)).has_one_sided_choice(2)
        );
        assert!(
            !Protocol::Resolved(Box::new(present.clone()), Box::new(present), 0)
                .has_one_sided_choice(2)
        );
    }

    fn trees(cost: usize, owned: bool) -> BTreeSet<BinaryShape> {
        if cost == 0 {
            let mut leaves = BTreeSet::from([BinaryShape::Leaf(OrdinaryLeaf::Exchange)]);
            if !owned {
                leaves.insert(BinaryShape::Leaf(OrdinaryLeaf::Converging));
            }
            return leaves;
        }
        let mut found = BTreeSet::new();
        for child in trees(cost - 1, owned) {
            found.insert(BinaryShape::Roll(Box::new(child.clone())));
            found.insert(BinaryShape::Receive(Box::new(child)));
        }
        for left in 0..cost {
            let right = cost - 1 - left;
            for a in trees(left, owned) {
                for b in trees(right, owned) {
                    found.insert(BinaryShape::Seq(Box::new(a.clone()), Box::new(b.clone())));
                    found.insert(BinaryShape::Route(Box::new(a.clone()), Box::new(b.clone())));
                    found.insert(BinaryShape::Resolved(Box::new(a.clone()), Box::new(b)));
                }
            }
            for a in trees(left, owned) {
                for b in trees(right, false) {
                    found.insert(BinaryShape::Par(Box::new(a.clone()), Box::new(b), false));
                }
            }
            if owned {
                for a in trees(left, false) {
                    for b in trees(right, true) {
                        found.insert(BinaryShape::Par(Box::new(a.clone()), Box::new(b), true));
                    }
                }
            }
        }
        found
    }

    fn converging(tree: &BinaryShape) -> bool {
        match tree {
            BinaryShape::Leaf(leaf) => *leaf == OrdinaryLeaf::Converging,
            BinaryShape::Seq(a, b)
            | BinaryShape::Par(a, b, _)
            | BinaryShape::Route(a, b)
            | BinaryShape::Resolved(a, b) => converging(a) || converging(b),
            BinaryShape::Roll(body) | BinaryShape::Receive(body) => converging(body),
        }
    }

    #[test]
    fn ranked_binary_populations_equal_independently_constructed_trees() {
        for owned in [false, true] {
            for budget in 0..=2 {
                let expected: BTreeSet<_> =
                    (0..=budget).flat_map(|cost| trees(cost, owned)).collect();
                let counts = binary_counts(budget);
                for population in [
                    Population::Exchange,
                    Population::Any,
                    Population::Converging,
                ] {
                    let actual: Vec<_> = (0..counts.total(owned, population))
                        .map(|rank| counts.shape(rank, owned, population))
                        .collect();
                    let unique: BTreeSet<_> = actual.iter().cloned().collect();
                    assert_eq!(unique.len(), actual.len(), "duplicate ranked tree");
                    let filtered: BTreeSet<_> = expected
                        .iter()
                        .filter(|tree| match population {
                            Population::Exchange => !converging(tree),
                            Population::Any => true,
                            Population::Converging => converging(tree),
                        })
                        .cloned()
                        .collect();
                    assert_eq!(
                        unique, filtered,
                        "owned={owned} budget={budget} population={population:?}"
                    );
                }
            }
        }
    }
}
