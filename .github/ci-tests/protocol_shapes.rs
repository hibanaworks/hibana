//! Generate protocol syntax and replay inputs independently of projection.
use std::{collections::BTreeSet, env, fmt::Write as _, fs, path::PathBuf};

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
    // Complete finite traces are derived from syntax, not accepted runtime calls.
    // Two visits per roll exercise re-entry; parallel children run in both orders.
    fn traces(&self, reverse: bool) -> Vec<(u32, Vec<u8>)> {
        match self {
            Self::Message { label, .. } => vec![(0, vec![*label])],
            Self::Seq(a, b) | Self::Par(a, b) => {
                let mut out = Vec::new();
                for (ca, ta) in a.traces(reverse) {
                    for (cb, tb) in b.traces(reverse) {
                        let trace = if reverse && matches!(self, Self::Par(..)) {
                            tb.iter().chain(&ta).copied().collect()
                        } else {
                            ta.iter().chain(&tb).copied().collect()
                        };
                        out.push((ca | cb, trace));
                    }
                }
                out
            }
            Self::Route(a, b) => a
                .traces(reverse)
                .into_iter()
                .chain(b.traces(reverse))
                .collect(),
            Self::Resolved(a, b, id) => a
                .traces(reverse)
                .into_iter()
                .chain(
                    b.traces(reverse)
                        .into_iter()
                        .map(|(c, t)| (c | (1 << id), t)),
                )
                .collect(),
            Self::Roll(a) => a
                .traces(reverse)
                .into_iter()
                .map(|(c, mut t)| {
                    t.extend_from_within(..);
                    (c, t)
                })
                .collect(),
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
impl Shape {
    fn build(&self, next: &mut u8, resolver: &mut u16, service: u8) -> Protocol {
        match self {
            Self::Loan => {
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
            Self::Seq(child) => {
                let prefix = message(next, 0, 1, false);
                Protocol::Seq(
                    Box::new(prefix),
                    Box::new(child.build(next, resolver, service)),
                )
            }
            Self::Par(child) => {
                let a = child.build(next, resolver, service);
                let b = message(next, 0, if service == 0 { 1 } else { service }, false);
                Protocol::Par(Box::new(a), Box::new(b))
            }
            Self::Route(a, b) => Protocol::Route(
                Box::new(a.build(next, resolver, service)),
                Box::new(b.build(next, resolver, service)),
            ),
            Self::Resolved(a, b) => {
                let id = *resolver;
                *resolver += 1;
                Protocol::Resolved(
                    Box::new(a.build(next, resolver, service)),
                    Box::new(b.build(next, resolver, service)),
                    id,
                )
            }
            Self::Roll(child) => Protocol::Roll(Box::new(child.build(next, resolver, service))),
        }
    }
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
    let mut all = Vec::new();
    all.extend(shapes(depth));
    let mut cases = Vec::new();
    let mut max_label = 0;
    let mut max_resolvers = 0;
    for (index, word) in all.iter().enumerate() {
        for (variant, service) in [2, 0].iter().enumerate() {
            let case = 2 * index + variant;
            if case % shards != shard {
                continue;
            }
            let mut count = 0;
            let mut resolvers = 0;
            let global = word.build(&mut count, &mut resolvers, *service);
            max_label = max_label.max(count);
            max_resolvers = max_resolvers.max(resolvers);
            cases.push((case, word, *service, global, resolvers));
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
    let driver = fs::read_to_string(root.join(".github/ci-tests/protocol_shape_driver.rs"))?;
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
    let (mut probes, mut trace_count, mut operations) = (0, 0, 0);
    let mut inputs = Vec::new();
    for (case, word, service, global, resolvers) in &cases {
        let mut messages = Vec::new();
        global.messages(&mut messages);
        let traces = global
            .traces(false)
            .into_iter()
            .chain(global.traces(true))
            .collect::<BTreeSet<_>>();
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
            "let global={};\nlet programs=Programs{{p0:project::<0>(&global),p1:project::<1>(&global),p2:project::<2>(&global),p3:project::<3>(&global),third_actor:{}}};",
            global.rust(),
            *service == 2
        )?;
        writeln!(
            source,
            "qualify(programs,{resolvers},{case},\"{word:?}/service={service}\",&["
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
                "Message{{from:{from},to:{to},label:{label},owned:{owned}}},"
            )?;
        }
        source.push_str("],&[\n");
        for choice in &patterns {
            let mut first = BTreeSet::new();
            global.first(*choice, &mut first);
            writeln!(
                source,
                "({choice},&{:?}),",
                first.into_iter().collect::<Vec<_>>()
            )?;
        }
        source.push_str("],&[\n");
        for (choice, trace) in &traces {
            writeln!(source, "({choice},&{trace:?}),")?;
            operations += 2 * trace.len();
        }
        source.push_str("]).await;\n}\n");
        probes += messages.len() * 2 * patterns.len();
        trace_count += traces.len();
    }
    fs::write(output.join("src/lib.rs"), source)?;
    fs::write(
        output.join("inputs.json"),
        format!("[{}]\n", inputs.join(",")),
    )?;
    println!(
        "generated shapes={} initial_probes={probes} complete_traces={trace_count} trace_operations={operations} depth={depth} shard={shard}/{shards} total_shapes={}",
        cases.len(),
        2 * all.len()
    );
    Ok(())
}
