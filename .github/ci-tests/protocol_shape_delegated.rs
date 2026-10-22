//! Generate operations performed through a received endpoint.
use super::{Protocol, message};

#[derive(Debug)]
pub(super) enum Body {
    Exchange,
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>),
    Route(Box<Self>, Box<Self>),
    Resolved(Box<Self>, Box<Self>),
    Roll(Box<Self>),
    Receive(Box<Self>),
    Forward(Box<Self>),
}

pub(super) struct Counts {
    local: Vec<usize>,
    mobile: Vec<usize>,
}
impl Counts {
    pub(super) fn new(budget: usize) -> Self {
        let mut counts = Self {
            local: vec![1],
            mobile: vec![1],
        };
        for cost in 1..=budget {
            let mut local = 2 * counts.local[cost - 1];
            let mut mobile = 3 * counts.mobile[cost - 1];
            for left in 0..cost {
                let right = cost - 1 - left;
                local += 4 * counts.local[left] * counts.local[right];
                mobile += 3 * counts.mobile[left] * counts.mobile[right]
                    + counts.local[left] * counts.local[right];
            }
            counts.local.push(local);
            counts.mobile.push(mobile);
        }
        counts
    }
    pub(super) fn total(&self) -> usize {
        self.mobile.iter().sum()
    }
    pub(super) fn shape(&self, mut rank: usize) -> Body {
        let mut cost = 0;
        while rank >= self.mobile[cost] {
            rank -= self.mobile[cost];
            cost += 1;
        }
        self.at(cost, rank, true)
    }
    fn at(&self, cost: usize, mut rank: usize, mobile: bool) -> Body {
        if cost == 0 {
            assert_eq!(rank, 0);
            return Body::Exchange;
        }
        let rows = if mobile { &self.mobile } else { &self.local };
        for operator in 0..if mobile { 3 } else { 2 } {
            if rank < rows[cost - 1] {
                let child = Box::new(self.at(cost - 1, rank, mobile));
                return match operator {
                    0 => Body::Roll(child),
                    1 => Body::Receive(child),
                    2 => Body::Forward(child),
                    _ => unreachable!(),
                };
            }
            rank -= rows[cost - 1];
        }
        for left in 0..cost {
            let right = cost - 1 - left;
            for operator in 0..4 {
                // A parallel borrow cannot move its parent endpoint. Each side
                // may contain arbitrary local operations, choices and rolls.
                let child_mobile = mobile && operator != 1;
                let children = if child_mobile {
                    &self.mobile
                } else {
                    &self.local
                };
                let width = children[right];
                let pairs = children[left] * width;
                if rank < pairs {
                    let a = Box::new(self.at(left, rank / width, child_mobile));
                    let b = Box::new(self.at(right, rank % width, child_mobile));
                    return match operator {
                        0 => Body::Seq(a, b),
                        1 => Body::Par(a, b),
                        2 => Body::Route(a, b),
                        3 => Body::Resolved(a, b),
                        _ => unreachable!(),
                    };
                }
                rank -= pairs;
            }
        }
        unreachable!("delegated body rank")
    }
}

impl Body {
    pub(super) fn build(&self, next: &mut u8, resolver: &mut u16, holder: u8) -> Protocol {
        match self {
            Self::Exchange => Protocol::Seq(
                Box::new(message(next, 3, 2, false)),
                Box::new(message(next, 2, 3, false)),
            ),
            Self::Seq(a, b) | Self::Par(a, b) => {
                let a = Box::new(a.build(next, resolver, holder));
                let b = Box::new(b.build(next, resolver, holder));
                if matches!(self, Self::Seq(..)) {
                    Protocol::Seq(a, b)
                } else {
                    Protocol::Par(a, b)
                }
            }
            Self::Route(a, b) | Self::Resolved(a, b) => {
                let id = *resolver;
                if matches!(self, Self::Resolved(..)) {
                    *resolver += 1;
                }
                let arm = |body: &Self, next: &mut u8, resolver: &mut u16| {
                    // The service and the other possible holder learn this
                    // choice before any optional onward delegation.
                    Protocol::Seq(
                        Box::new(message(next, 3, 2, false)),
                        Box::new(Protocol::Seq(
                            Box::new(message(next, 3, holder ^ 1, false)),
                            Box::new(body.build(next, resolver, holder)),
                        )),
                    )
                };
                let a = Box::new(arm(a, next, resolver));
                let b = Box::new(arm(b, next, resolver));
                if matches!(self, Self::Resolved(..)) {
                    Protocol::Resolved(a, b, id)
                } else {
                    Protocol::Route(a, b)
                }
            }
            Self::Roll(body) => Protocol::Roll(Box::new(body.build(next, resolver, holder))),
            Self::Receive(body) => Protocol::Seq(
                Box::new(message(next, 2, 3, false)),
                Box::new(Protocol::Seq(
                    Box::new(message(next, 3, 2, false)),
                    Box::new(body.build(next, resolver, holder)),
                )),
            ),
            Self::Forward(body) => Protocol::Seq(
                Box::new(message(next, holder, holder ^ 1, true)),
                Box::new(Protocol::Seq(
                    Box::new(body.build(next, resolver, holder ^ 1)),
                    Box::new(message(next, holder ^ 1, holder, true)),
                )),
            ),
        }
    }
    pub(super) fn global(&self, next: &mut u8, resolver: &mut u16) -> Protocol {
        Protocol::Seq(
            Box::new(message(next, 0, 1, true)),
            Box::new(Protocol::Seq(
                Box::new(self.build(next, resolver, 1)),
                Box::new(message(next, 1, 0, true)),
            )),
        )
    }
}

fn parallel_parts<'a>(source: &'a Protocol, out: &mut Vec<&'a Protocol>) {
    if let Protocol::Par(a, b) = source {
        parallel_parts(a, out);
        parallel_parts(b, out);
    } else {
        out.push(source);
    }
}

fn roles(source: &Protocol) -> [bool; 4] {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let mut roles = [false; 4];
    for message in messages {
        let Protocol::Message {
            from, to, owned, ..
        } = message
        else {
            unreachable!()
        };
        assert!(
            !owned,
            "onward delegation cannot occur while its endpoint is borrowed"
        );
        roles[usize::from(*from)] = true;
        roles[usize::from(*to)] = true;
    }
    roles
}

fn first_local(source: &Protocol, actor: u8, holder: u8) -> (u8, u8) {
    match source {
        Protocol::Message {
            from, to, label, ..
        } => {
            let physical = |role| if role == 3 { holder } else { role };
            if physical(*from) == actor {
                (*from, *label)
            } else {
                assert_eq!(physical(*to), actor);
                (*to, *label)
            }
        }
        Protocol::Seq(a, b) => {
            let mut messages = Vec::new();
            a.messages(&mut messages);
            let present = messages.iter().any(|m| match m {
                Protocol::Message { from, to, .. } => [*from, *to]
                    .into_iter()
                    .any(|r| (if r == 3 { holder } else { r }) == actor),
                _ => unreachable!(),
            });
            first_local(if present { a } else { b }, actor, holder)
        }
        _ => panic!("choice arms must start with explicit notifications"),
    }
}

/// Emit direct public endpoint operations, including every nested par borrow.
/// The source is test input. No runtime result selects a schedule or an owner.
pub(super) fn local_rust(source: &Protocol, actor: u8, initial: u8) -> String {
    fn emit(
        source: &Protocol,
        actor: u8,
        holder: &mut u8,
        names: &[String; 4],
        choices: &mut usize,
    ) -> String {
        match source {
            Protocol::Message {
                from,
                to,
                label,
                owned,
            } => {
                let physical = |role| if role == 3 { *holder } else { role };
                let mut code = String::new();
                if physical(*from) == actor {
                    code += &format!(
                        "{}.send::<M{label}<'_>>({}).await?;",
                        names[usize::from(*from)],
                        if *owned { "db" } else { "&()" }
                    );
                }
                if physical(*to) == actor {
                    code += &format!(
                        "{}{}.recv::<M{label}<'_>>().await?;",
                        if *owned { "db=" } else { "" },
                        names[usize::from(*to)]
                    );
                }
                if *owned {
                    assert_eq!(*holder, *from);
                    *holder = *to;
                }
                code
            }
            Protocol::Seq(a, b) => {
                emit(a, actor, holder, names, choices) + &emit(b, actor, holder, names, choices)
            }
            Protocol::Roll(body) => {
                let before = *holder;
                let body = emit(body, actor, holder, names, choices);
                assert_eq!(*holder, before);
                if body.is_empty() {
                    body
                } else {
                    format!("for _ in 0..2{{{body}}}")
                }
            }
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                let bit = *choices;
                *choices += 1;
                let before = *holder;
                let left = emit(a, actor, holder, names, choices);
                let after = *holder;
                *holder = before;
                let right = emit(b, actor, holder, names, choices);
                assert_eq!(*holder, after);
                if left.is_empty() && right.is_empty() {
                    return String::new();
                }
                let (view, left_label) = first_local(a, actor, before);
                let (right_view, right_label) = first_local(b, actor, before);
                assert_eq!(view, right_view);
                if actor == before && matches!(source, Protocol::Route(..)) {
                    format!("if choice & (1<<{bit})==0{{{left}}}else{{{right}}}")
                } else {
                    format!(
                        "{{let branch={}.offer().await?;let label=branch.label();drop(branch);match label{{{left_label}=>{{{left}}},{right_label}=>{{{right}}},_=>panic!(\"unexpected generated branch {{label}}\")}}}}",
                        names[usize::from(view)]
                    )
                }
            }
            Protocol::Par(..) => {
                let mut parts = Vec::new();
                parallel_parts(source, &mut parts);
                let memberships = parts.iter().map(|p| roles(p)).collect::<Vec<_>>();
                let mut part_names = vec![names.clone(); parts.len()];
                let mut declarations = String::new();
                for role in 0..4 {
                    if (if role == 3 {
                        usize::from(*holder)
                    } else {
                        role
                    }) != usize::from(actor)
                    {
                        continue;
                    }
                    let indices = (0..parts.len())
                        .filter(|i| memberships[*i][role])
                        .collect::<Vec<_>>();
                    if indices.is_empty() {
                        continue;
                    }
                    let bindings = indices
                        .iter()
                        .map(|i| {
                            let name = format!("p{role}_{i}");
                            part_names[*i][role] = name.clone();
                            format!("mut {name}")
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    declarations += &format!("let [{bindings}]={}.par()?;", names[role]);
                }
                let mut futures = Vec::new();
                for (i, part) in parts.into_iter().enumerate() {
                    let mut local_holder = *holder;
                    let body = emit(part, actor, &mut local_holder, &part_names[i], choices);
                    assert_eq!(local_holder, *holder);
                    if !body.is_empty() {
                        futures.push(format!("async{{{body}Ok::<_,hibana::EndpointError>(())}}"));
                    }
                }
                match futures.len() {
                    0 => String::new(),
                    1 => format!("{{{declarations}{}.await?;}}", futures[0]),
                    _ => format!(
                        "{{{declarations}tokio::try_join!({})?;}}",
                        futures.join(",")
                    ),
                }
            }
        }
    }
    let mut holder = initial;
    emit(
        source,
        actor,
        &mut holder,
        &["e0".into(), "e1".into(), "e2".into(), "db".into()],
        &mut 0,
    )
}

pub(super) fn resolver_bits(source: &Protocol) -> (usize, Vec<(u16, usize)>) {
    fn collect(source: &Protocol, bit: &mut usize, resolvers: &mut Vec<(u16, usize)>) {
        match source {
            Protocol::Message { .. } => (),
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                if let Protocol::Resolved(_, _, id) = source {
                    resolvers.push((*id, *bit));
                }
                *bit += 1;
                collect(a, bit, resolvers);
                collect(b, bit, resolvers);
            }
            Protocol::Seq(a, b) | Protocol::Par(a, b) => {
                collect(a, bit, resolvers);
                collect(b, bit, resolvers);
            }
            Protocol::Roll(body) => collect(body, bit, resolvers),
        }
    }
    let mut bits = 0;
    let mut resolvers = Vec::new();
    collect(source, &mut bits, &mut resolvers);
    (bits, resolvers)
}

pub(super) fn concurrent_rust(source: &Protocol, owner: u8) -> String {
    let (bits, resolvers) = resolver_bits(source);
    let mut code = format!(
        r#"
    for {} in 0..{}u32 {{
        let sid=SessionId::new(2);
        // SAFETY: each execution owns its resource and its only initial issuer.
        let network=InMemoryTransport::new([((sid,3),unsafe{{EndpointBinding::new()}})]);
        let [mut slab0,mut slab1,mut slab2]=[vec![0;128*1024],vec![0;128*1024],vec![0;128*1024]];
        let mut s0=SessionKitStorage::uninit();let mut s1=SessionKitStorage::uninit();let mut s2=SessionKitStorage::uninit();
        let rv0=s0.init().rendezvous(&mut slab0,network.clone()).unwrap();
        let rv1=s1.init().rendezvous(&mut slab1,network.clone()).unwrap();
        let rv2=s2.init().rendezvous(&mut slab2,network.clone()).unwrap();
    "#,
        if bits == 0 { "_" } else { "choice" },
        1usize << bits
    );
    for (id, bit) in resolvers {
        for actor in 0..2 {
            code += &format!(
                "rv{actor}.set_resolver(programs.p3.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&choice,|choice|Ok(if choice & (1<<{bit})==0{{DecisionArm::Left}}else{{DecisionArm::Right}}))).unwrap();"
            );
        }
    }
    code += &format!(
        r#"
        let mut e0=rv0.enter(sid,&programs.p0).unwrap();let mut e1=rv1.enter(sid,&programs.p1).unwrap();let mut e2=rv2.enter(sid,&programs.p2).unwrap();
        let initial_db=rv{owner}.enter(sid,programs.p3.as_ref().unwrap()).unwrap();
    "#
    );
    for actor in 0..3 {
        let db = if actor == owner {
            "let mut db=initial_db;"
        } else if actor == owner ^ 1 {
            "let mut db;"
        } else {
            ""
        };
        code += &format!(
            "let run{actor}=async{{{db}{}{}Ok::<_,hibana::EndpointError>(())}};",
            local_rust(source, actor, owner),
            if actor == owner { "drop(db);" } else { "" }
        );
    }
    code += r#"
        tokio::time::timeout(core::time::Duration::from_secs(1),async{tokio::try_join!(run0,run1,run2)}).await.expect("generated delegated localside stalled").unwrap();
    }
    "#;
    code
}
