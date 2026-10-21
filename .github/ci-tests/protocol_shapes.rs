//! Generate protocol syntax and replay inputs independently of projection.
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    path::PathBuf,
};

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Trace {
    messages: Vec<(u8, bool)>,
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
#[derive(Clone)]
enum Protocol {
    Message {
        from: u8,
        to: u8,
        label: u8,
        owned: bool,
    },
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>, u16),
    Roll(Box<Self>),
}
impl Protocol {
    fn rust(&self) -> String {
        match self {
            Self::Message {
                from, to, label, ..
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
                label,
                owned,
            } => format!(
                r#"{{"kind":0,"left":0,"right":0,"sender":{from},"receiver":{to},"label":{label},"owned":{owned},"resolver":0}}"#
            ),
            Self::Roll(body) => {
                let left = body.json_nodes(out);
                format!(
                    r#"{{"kind":5,"left":{left},"right":0,"sender":0,"receiver":0,"label":0,"owned":false,"resolver":0}}"#
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
                    r#"{{"kind":{kind},"left":{left},"right":{right},"sender":0,"receiver":0,"label":0,"owned":false,"resolver":{resolver}}}"#
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
    fn first(&self, choice: u32, labels: &mut BTreeSet<u8>) {
        match self {
            Self::Message { label, .. } => {
                labels.insert(*label);
            }
            Self::Seq(a, _) | Self::Roll(a) => a.first(choice, labels),
            Self::Resolved(a, b, id) => {
                if choice & (1 << id) == 0 {
                    a.first(choice, labels)
                } else {
                    b.first(choice, labels)
                }
            }
            Self::Par(a, b) | Self::Route(a, b) => {
                a.first(choice, labels);
                b.first(choice, labels);
            }
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
    fn parallel_groups(&self) -> Vec<Vec<u8>> {
        fn collect(protocol: &Protocol, groups: &mut Vec<Vec<u8>>) {
            match protocol {
                Protocol::Par(a, b) => {
                    collect(a, groups);
                    collect(b, groups);
                }
                _ => {
                    let mut messages = Vec::new();
                    protocol.messages(&mut messages);
                    groups.push(
                        messages
                            .into_iter()
                            .map(|m| {
                                let Protocol::Message { label, .. } = m else {
                                    unreachable!()
                                };
                                *label
                            })
                            .collect(),
                    );
                }
            }
        }
        let mut groups = Vec::new();
        if matches!(self, Self::Par(..)) {
            collect(self, &mut groups);
        }
        groups
    }
    fn resolver_choices(
        &self,
        path: &mut Vec<(u16, bool)>,
        out: &mut BTreeMap<u8, Vec<(u16, bool)>>,
    ) {
        match self {
            Self::Message { label, .. } => {
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
    // Complete finite traces are derived from syntax, not accepted runtime calls.
    // Two visits per roll exercise re-entry; parallel children run in both orders.
    fn traces(&self) -> Vec<Trace> {
        match self {
            Self::Message { label, .. } => vec![Trace {
                messages: vec![(*label, false)],
            }],
            Self::Seq(a, b) | Self::Par(a, b) => {
                let left = a.traces();
                let right = b.traces();
                let mut out = Vec::new();
                for a in &left {
                    for b in &right {
                        out.push(a.followed_by(b));
                        if matches!(self, Self::Par(..)) {
                            out.push(b.followed_by(a));
                        }
                    }
                }
                out
            }
            Self::Route(a, b) | Self::Resolved(a, b, _) => a
                .traces()
                .into_iter()
                .chain(b.traces())
                .map(|mut trace| {
                    trace.messages.first_mut().unwrap().1 = true;
                    trace
                })
                .collect(),
            Self::Roll(body) => {
                let visits = body.traces();
                let mut out = Vec::new();
                for first in &visits {
                    for second in &visits {
                        out.push(first.followed_by(second));
                    }
                }
                out
            }
        }
    }
}

fn message(next: &mut u8, from: u8, to: u8, owned: bool) -> Protocol {
    let label = *next;
    *next = next.checked_add(1).expect("message-label domain");
    Protocol::Message {
        from,
        to,
        label,
        owned,
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
#[derive(Clone, Copy, Debug)]
enum OrdinaryLeaf {
    Message,
    Exchange,
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
            OrdinaryLeaf::Exchange => 1,
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
#[derive(Debug)]
enum CaseShape {
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 5 {
        return Err("usage: protocol-shapes REPOSITORY OUTPUT DEPTH [SHARD SHARDS]".into());
    }
    let root = PathBuf::from(&args[0]).canonicalize()?;
    let output = PathBuf::from(&args[1]);
    let depth: usize = args[2].parse()?;
    if depth > 5 {
        return Err("depth exceeds message-label domain".into());
    }
    let (shard, shards) = if args.len() == 5 {
        (args[3].parse::<usize>()?, args[4].parse::<usize>()?)
    } else {
        (0, 1)
    };
    if shards == 0 || shard >= shards {
        return Err("invalid shard".into());
    }
    fs::create_dir_all(output.join("src"))?;
    let trace_directory = output.join("traces");
    if trace_directory.exists() {
        fs::remove_dir_all(&trace_directory)?;
    }
    fs::create_dir_all(&trace_directory)?;
    fs::write(
        output.join("Cargo.toml"),
        format!(
            r#"[package]
name = "hibana-generated-protocol-shapes-{depth}-{shard}"
version = "0.0.0"
edition = "2024"
publish = false
[workspace]
[dependencies]
hibana = {{ path = {:?} }}
tokio = {{ version = "=1.48.0", features = ["macros", "rt", "time", "test-util"] }}
[lints.rust]
long_running_const_eval = "forbid"
"#,
            root
        ),
    )?;
    let all = catalog(depth);
    let mut cases = Vec::new();
    let mut max_label = 0;
    let mut max_resolvers = 0;
    for (index, word) in all.iter().enumerate() {
        for (variant, (service, swap)) in [(2, false), (0, false), (2, true), (0, true)]
            .into_iter()
            .enumerate()
        {
            let case = 4 * index + variant;
            if case % shards != shard {
                continue;
            }
            let mut count = 0;
            let mut resolvers = 0;
            let mut global = word.build(&mut count, &mut resolvers, service);
            if swap {
                global.swap_participants();
            }
            max_label = max_label.max(count);
            max_resolvers = max_resolvers.max(resolvers);
            cases.push((case, word, service, u8::from(swap), global, resolvers));
        }
    }
    if cases.is_empty() {
        return Err("empty shard".into());
    }
    let mut source = format!(
        "#![cfg(test)]\n#![forbid(long_running_const_eval)]\n#[path={:?}]\nmod in_memory;\n",
        root.join("examples/support/in_memory.rs")
    );
    let registers = (0..max_resolvers)
        .map(|id| format!("register!({id});\n"))
        .collect::<String>();
    let dispatch = (0..max_label)
        .map(|label| format!("{label} => execute!({label}),\n"))
        .collect::<String>();
    let max_width = cases
        .iter()
        .map(|(_, _, _, _, g, _)| g.parallel_groups().len())
        .max()
        .unwrap()
        .max(2);
    let splits = (2..=max_width).map(|width| format!(
        "{width} => endpoint.par::<{width}>().unwrap().into_iter().map(Fragment::Parallel).collect(),\n"
    )).collect::<String>();
    let joins = (2..=max_width).map(|width| {
        let declarations = (0..width).map(|i| format!("let f{i} = work.next().unwrap();")).collect::<String>();
        let arguments = (0..width).map(|i| format!("f{i}")).collect::<Vec<_>>().join(",");
        let results = (0..width).map(|i| format!("outputs.{i}")).collect::<Vec<_>>().join(",");
        format!("{width} => {{ {declarations} let outputs = tokio::join!({arguments}); vec![{results}] }},\n")
    }).collect::<String>();
    let mut driver = fs::read_to_string(root.join(".github/ci-tests/protocol_shape_driver.rs"))?;
    driver.push_str(&fs::read_to_string(
        root.join(".github/ci-tests/protocol_shape_parallel_driver.rs"),
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
            .replace("0 => dispatch_labels!(),", &dispatch)
            .replace("2 => split_widths!(),", &splits)
            .replace("2 => join_widths!(),", &joins),
    );
    let (mut probes, mut trace_count, mut operations, mut concurrent_traces) = (0, 0, 0, 0);
    let mut inputs = Vec::new();
    for (case, word, service, owner, global, resolvers) in &cases {
        let mut messages = Vec::new();
        global.messages(&mut messages);
        let traces = global.traces().into_iter().collect::<BTreeSet<_>>();
        let groups = global.parallel_groups();
        let mut decisions = BTreeMap::new();
        global.resolver_choices(&mut Vec::new(), &mut decisions);
        let patterns = (0..(1u32 << resolvers)).collect::<Vec<_>>();
        let mut nodes = Vec::new();
        global.json_nodes(&mut nodes);
        inputs.push(format!(
            r#"{{"name":{case},"nodes":[{}],"resolvers":{resolvers}}}"#,
            nodes.join(",")
        ));
        writeln!(
            source,
            "#[tokio::test(flavor=\"current_thread\",start_paused=true)]\nasync fn shape_{case}() {{"
        )?;
        for m in &messages {
            let Protocol::Message { label, owned, .. } = m else {
                unreachable!()
            };
            let payload = if *owned { "Endpoint<'r,3>" } else { "()" };
            writeln!(source, "type M{label}<'r>=g::Msg<{label},{payload}>;")?;
        }
        writeln!(
            source,
            "let global={};\nlet programs=Programs{{p0:project::<0>(&global),p1:project::<1>(&global),p2:project::<2>(&global),p3:project::<3>(&global),third_actor:{},owner:{owner}}};",
            global.rust(),
            *service == 2
        )?;
        writeln!(
            source,
            "qualify(programs,Scenario{{resolver_count:{resolvers},id:{case},name:\"{word:?}/service={service}/owner={owner}\",messages:&["
        )?;
        for m in &messages {
            let Protocol::Message {
                from,
                to,
                label,
                owned,
            } = m
            else {
                unreachable!()
            };
            writeln!(
                source,
                "Message{{from:{from},to:{to},label:{label},owned:{owned},decisions:&{:?}}},",
                decisions[label]
            )?;
        }
        source.push_str("],initial:&[\n");
        for choice in &patterns {
            let mut first = BTreeSet::new();
            global.first(*choice, &mut first);
            writeln!(
                source,
                "({choice},&{:?}),",
                first.into_iter().collect::<Vec<_>>()
            )?;
        }
        let mut trace_data = String::new();
        for trace in &traces {
            trace_data.push_str(&trace.line());
            operations += 2 * trace.messages.len();
        }
        fs::write(
            output.join("traces").join(format!("{case}.txt")),
            trace_data,
        )?;
        writeln!(
            source,
            "],traces:include_str!(\"../traces/{case}.txt\"),expected_traces:{},parallel_groups:&[{}]}}).await;\n}}",
            traces.len(),
            groups
                .iter()
                .map(|labels| format!("&{labels:?}"))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        probes += messages.len() * 2 * patterns.len();
        trace_count += traces.len();
        if !groups.is_empty() {
            concurrent_traces += traces.len();
        }
    }
    fs::write(output.join("src/lib.rs"), source)?;
    fs::write(
        output.join("inputs.json"),
        format!("[{}]\n", inputs.join(",")),
    )?;
    println!(
        "generated shapes={} initial_probes={probes} complete_traces={trace_count} trace_operations={operations} concurrent_traces={concurrent_traces} depth={depth} shard={shard}/{shards} total_shapes={}",
        cases.len(),
        4 * all.len()
    );
    Ok(())
}
