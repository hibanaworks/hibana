//! Emit typed localsides from the same immutable syntax as the test input.
use super::{Execution, Protocol};

pub(super) fn contains(source: &Protocol, label: u8) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { event: candidate, .. } if *candidate == label))
}

fn bounds(source: &Protocol) -> (usize, usize) {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    let first = match messages.first().unwrap() {
        Protocol::Message { event, .. } => usize::from(*event),
        _ => unreachable!(),
    };
    let last = match messages.last().unwrap() {
        Protocol::Message { event, .. } => usize::from(*event) + 1,
        _ => unreachable!(),
    };
    (first, last)
}

fn borrow_observation(group: &Protocol, part: &Protocol, actor: u8, role: usize) -> String {
    let (start, stop) = bounds(group);
    let (part_start, part_stop) = bounds(part);
    format!(
        "observed.borrows.borrow_mut().push([{actor},{role},{start},{stop},{part_start},{part_stop},script.first_occurrence(),observed.completed.borrow().len()]);"
    )
}

pub(super) fn message_selected_head(source: &Protocol, label: u8) -> bool {
    match source {
        Protocol::Message { .. } => false,
        Protocol::Route(..) => true,
        Protocol::Seq(first, _) | Protocol::Roll(first) => message_selected_head(first, label),
        Protocol::Par(left, right) | Protocol::Resolved(left, right, _) => {
            message_selected_head(if contains(left, label) { left } else { right }, label)
        }
    }
}

pub(super) fn mobile(source: &Protocol) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { owned: true, .. }))
}

// An incoming continuation inside a choice remains offerable after a rolled
// prefix. Exercise that public path with a live Future, not a cancelled
// one-poll probe or only a preview whose frame was already queued.
fn follows_roll_in_choice(source: &Protocol, event: u8) -> bool {
    fn rolled(source: &Protocol) -> bool {
        match source {
            Protocol::Roll(_) => true,
            Protocol::Message { .. } => false,
            Protocol::Seq(a, b)
            | Protocol::Par(a, b)
            | Protocol::Route(a, b)
            | Protocol::Resolved(a, b, _) => rolled(a) || rolled(b),
        }
    }
    fn walk(source: &Protocol, event: u8, choice: bool, after: bool) -> bool {
        match source {
            Protocol::Message { event: found, .. } => *found == event && choice && after,
            Protocol::Seq(a, b) => {
                walk(a, event, choice, after) || walk(b, event, choice, after || rolled(a))
            }
            Protocol::Par(a, b) => walk(a, event, choice, after) || walk(b, event, choice, after),
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                walk(a, event, true, after) || walk(b, event, true, after)
            }
            Protocol::Roll(body) => walk(body, event, choice, after),
        }
    }
    walk(source, event, false, false)
}

pub(super) fn holder_after(source: &Protocol, execution: &Execution, holder: u8) -> u8 {
    match (source, execution) {
        (
            Protocol::Message {
                from,
                to,
                owned: true,
                ..
            },
            Execution::Message(_),
        ) => {
            assert_eq!(holder, *from);
            *to
        }
        (Protocol::Message { .. }, Execution::Message(_)) => holder,
        (Protocol::Seq(a, b), Execution::Seq(left, right)) => {
            holder_after(b, right, holder_after(a, left, holder))
        }
        (Protocol::Par(a, b), Execution::Par(left, right, _)) => {
            assert!(!mobile(a) || !mobile(b));
            if mobile(a) {
                holder_after(a, left, holder)
            } else {
                holder_after(b, right, holder)
            }
        }
        (Protocol::Route(a, b) | Protocol::Resolved(a, b, _), Execution::Branch(body, right)) => {
            holder_after(if *right { b } else { a }, body, holder)
        }
        (Protocol::Roll(body), Execution::Roll(first, second)) => {
            holder_after(body, second, holder_after(body, first, holder))
        }
        _ => panic!("source and ownership execution disagree"),
    }
}

// Generate only scripted input data. A callback can preview the next parent
// visit before a retained child finishes; resetting every ancestor to the
// child's historical path would contradict that already parked decision.
// Each resolver's planned choices form its input prefix. The last value is
// held thereafter, so previewing beyond the finite execution still has input.
// A resolver absent from the entire script has no input and must not run.
fn resolver_input_code(source: &Protocol) -> String {
    match source {
        Protocol::Message { .. } => String::new(),
        Protocol::Seq(a, b) | Protocol::Par(a, b) => {
            let (a, b) = (resolver_input_code(a), resolver_input_code(b));
            if a.is_empty() && b.is_empty() {
                return String::new();
            }
            let pattern = if matches!(source, Protocol::Seq(..)) {
                "Script::Seq(a,b)"
            } else {
                "Script::Par(a,b,_)"
            };
            format!(
                "{{let(a,b)=match script{{{pattern}=>(a.as_ref(),b.as_ref()),_=>panic!(\"source/input pair mismatch\")}};{{let script=a;{}}}{{let script=b;{}}}}}",
                a, b
            )
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let (a, b) = (resolver_input_code(a), resolver_input_code(b));
            let input = match source {
                Protocol::Resolved(_, _, id) => format!("decision_inputs[{id}].push_back(right);"),
                _ => String::new(),
            };
            if input.is_empty() && a.is_empty() && b.is_empty() {
                return String::new();
            }
            format!(
                "{{let(script,right)=match script{{Script::Branch(body,right)=>(body.as_ref(),*right),_=>panic!(\"source/input branch mismatch\")}};{input}if right{{{}}}else{{{}}}}}",
                b, a
            )
        }
        Protocol::Roll(body) => {
            let inputs = resolver_input_code(body);
            if inputs.is_empty() {
                return inputs;
            }
            format!(
                "{{for visit in match script{{Script::Roll(visits)=>visits,_=>panic!(\"source/input visit mismatch\")}}{{let script=visit.as_ref();{inputs}}}}}"
            )
        }
    }
}

pub(super) fn present(source: &Protocol, role: u8, primed: Option<u8>) -> bool {
    match source {
        Protocol::Message {
            from,
            to,
            event: label,
            ..
        } => {
            [*from, *to].contains(&role) && (primed != Some(*label) || (*to == role && from != to))
        }
        Protocol::Seq(a, b) | Protocol::Par(a, b) => {
            present(a, role, primed) || present(b, role, primed)
        }
        Protocol::Roll(body) => present(body, role, None),
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => match primed {
            Some(label) if contains(a, label) => present(a, role, primed),
            Some(label) if contains(b, label) => present(b, role, primed),
            _ => present(a, role, None) || present(b, role, None),
        },
    }
}

pub(super) fn parallel_parts<'a>(source: &'a Protocol, parts: &mut Vec<&'a Protocol>) {
    if let Protocol::Par(a, b) = source {
        parallel_parts(a, parts);
        parallel_parts(b, parts);
    } else {
        parts.push(source);
    }
}

fn parallel_prefix(source: &Protocol, role: u8) -> bool {
    match source {
        Protocol::Par(..) => true,
        Protocol::Seq(first, tail) if !present(tail, role, None) => parallel_prefix(first, role),
        _ => false,
    }
}

/// Local leaves of an ordinary-view group, retaining real sequential tails
/// and roll boundaries. A group using the resource view retains its separate
/// view borrows and source parts, as its emitted localside drives both views.
pub(super) fn parallel_parts_for_actor<'a>(
    source: &'a Protocol,
    actor: u8,
    parts: &mut Vec<&'a Protocol>,
) {
    fn collect<'a>(source: &'a Protocol, role: u8, parts: &mut Vec<&'a Protocol>) {
        match source {
            Protocol::Par(a, b) => {
                collect(a, role, parts);
                collect(b, role, parts);
            }
            Protocol::Seq(first, tail)
                if !present(tail, role, None) && parallel_prefix(first, role) =>
            {
                collect(first, role, parts)
            }
            _ => parts.push(source),
        }
    }
    if present(source, 3, None) {
        parallel_parts(source, parts);
    } else {
        collect(source, actor, parts);
    }
}

fn parallel_ancestors<'a>(source: &'a Protocol, target: &Protocol) -> Option<Vec<&'a Protocol>> {
    if source == target {
        return Some(Vec::new());
    }
    match source {
        Protocol::Message { .. } => None,
        Protocol::Par(..) => {
            let mut parts = Vec::new();
            parallel_parts(source, &mut parts);
            parts.into_iter().find_map(|part| {
                let mut path = parallel_ancestors(part, target)?;
                path.insert(0, source);
                Some(path)
            })
        }
        Protocol::Seq(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            parallel_ancestors(a, target).or_else(|| parallel_ancestors(b, target))
        }
        Protocol::Roll(body) => parallel_ancestors(body, target),
    }
}

/// The whole payload value is moved into a mobile parallel child. Other
/// children can hold borrowed resource fragments, but cannot move that value.
pub(super) fn payload_in_scope(global: &Protocol, target: &Protocol) -> bool {
    parallel_ancestors(global, target)
        .expect("payload query must belong to the source")
        .into_iter()
        .all(|parent| {
            let mut parts = Vec::new();
            parallel_parts(parent, &mut parts);
            parts
                .into_iter()
                .any(|part| mobile(part) && parallel_ancestors(part, target).is_some())
        })
}

fn borrows_resource_mutably(source: &Protocol, actor: u8) -> bool {
    match source {
        Protocol::Message {
            from, to, owned, ..
        } => {
            if *owned {
                [*from, *to].contains(&actor)
            } else {
                [*from, *to].contains(&3)
            }
        }
        Protocol::Par(..) if mobile(source) => true,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => {
            borrows_resource_mutably(a, actor) || borrows_resource_mutably(b, actor)
        }
        Protocol::Roll(body) => borrows_resource_mutably(body, actor),
    }
}

pub(super) fn first_local(source: &Protocol, actor: u8, holder: u8) -> Vec<(u8, u8)> {
    let physical = |role| if role == 3 { holder } else { role };
    match source {
        Protocol::Message {
            from,
            to,
            event: label,
            ..
        } => {
            if physical(*from) == actor {
                vec![(*from, *label)]
            } else if physical(*to) == actor {
                vec![(*to, *label)]
            } else {
                vec![]
            }
        }
        Protocol::Seq(a, b) => {
            let first = first_local(a, actor, holder);
            if first.is_empty() {
                first_local(b, actor, holder)
            } else {
                first
            }
        }
        Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let mut first = first_local(a, actor, holder);
            first.extend(first_local(b, actor, holder));
            first
        }
        Protocol::Roll(body) => first_local(body, actor, holder),
    }
}

fn heads<'a>(source: &'a Protocol, out: &mut Vec<&'a Protocol>) {
    match source {
        Protocol::Message { .. } => out.push(source),
        Protocol::Seq(a, _) | Protocol::Roll(a) => heads(a, out),
        Protocol::Par(a, b) | Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            heads(a, out);
            heads(b, out);
        }
    }
}

/// Peel only sequential syntax. Crossing a parallel or repeated node here
/// would impose an order or consume a visit that the source does not require.
pub(super) fn split_first(source: &Protocol) -> Option<(&Protocol, Option<Protocol>)> {
    match source {
        Protocol::Message { .. } => Some((source, None)),
        Protocol::Seq(a, b) => {
            let (first, rest) = split_first(a)?;
            Some((
                first,
                Some(match rest {
                    Some(rest) => Protocol::Seq(Box::new(rest), b.clone()),
                    None => *b.clone(),
                }),
            ))
        }
        _ => None,
    }
}

pub(super) fn same_message(a: &Protocol, b: &Protocol) -> bool {
    matches!((a, b), (
        Protocol::Message { from: af, to: at, contract: ac, owned: ao, .. },
        Protocol::Message { from: bf, to: bt, contract: bc, owned: bo, .. },
    ) if (af, at, ac, ao) == (bf, bt, bc, bo))
}

struct Emitter<'a> {
    source: &'a Protocol,
    paths: std::collections::BTreeMap<u8, Vec<(u16, bool)>>,
    causal: Vec<(u8, u8, u8)>,
    route_owners: Vec<(&'a Protocol, u8)>,
}

// Immutable source analysis for code generation. A route needs two ownership
// variants only when different preceding source paths really leave the
// resource with different actors. This is not emitted as application state.
fn route_owners<'a>(source: &'a Protocol, incoming: u8, out: &mut Vec<(&'a Protocol, u8)>) -> u8 {
    match source {
        Protocol::Message {
            from,
            to,
            owned: true,
            ..
        } => {
            assert_eq!(incoming, 1 << from);
            1 << to
        }
        Protocol::Message { .. } => incoming,
        Protocol::Seq(a, b) => {
            let middle = route_owners(a, incoming, out);
            route_owners(b, middle, out)
        }
        Protocol::Par(a, b) => {
            let left = route_owners(a, incoming, out);
            let right = route_owners(b, incoming, out);
            assert!(!mobile(a) || !mobile(b));
            if mobile(a) { left } else { right }
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            out.push((source, incoming));
            route_owners(a, incoming, out) | route_owners(b, incoming, out)
        }
        Protocol::Roll(body) => {
            assert_eq!(route_owners(body, incoming, out), incoming);
            incoming
        }
    }
}

impl Emitter<'_> {
    // Only the deliberately invalid, single-poll measurement is shared.
    // Successful localside operations remain directly in their source bodies.
    // Otherwise every before/after cut duplicates this complete selector
    // dispatch in the same async Future, causing quadratic generated code.
    fn query_dispatch(&self) -> String {
        let mut code = String::from(
            "trait QueryEndpoint<'r>{fn query(&mut self,db:&mut Option<Endpoint<'r,3>>,input:(u8,bool,&mut core::task::Context<'_>))->Result<Outcome,hibana::EndpointError>;}",
        );
        for role in 0..4 {
            let mut arms = String::new();
            for (query, receive) in super::scope_probes::public_queries(self.source, role, Some(3))
            {
                let Protocol::Message { event, owned, .. } = query else {
                    unreachable!()
                };
                let call = if receive {
                    format!("self.recv::<M{event}>()")
                } else {
                    format!(
                        "self.send::<M{event}>({})",
                        if *owned {
                            "db.take().expect(\"owned query has no Endpoint value\")"
                        } else {
                            "&()"
                        }
                    )
                };
                arms += &format!(
                    "({event},{receive})=>{{let mut future=core::pin::pin!({call});match future.as_mut().poll(cx){{core::task::Poll::Ready(Ok(_))=>Ok(Outcome::Accepted),core::task::Poll::Ready(Err(error))=>Err(error),core::task::Poll::Pending=>Ok(Outcome::Pending)}}}},"
                );
            }
            if arms.is_empty() {
                continue;
            }
            for (lifetimes, target) in [
                ("'r", format!("Endpoint<'r,{role}>")),
                ("'e,'r", format!("hibana::EndpointBorrow<'e,'r,{role}>")),
            ] {
                code += &format!(
                    "impl<{lifetimes}> QueryEndpoint<'r> for {target}{{fn query(&mut self,db:&mut Option<Endpoint<'r,3>>,input:(u8,bool,&mut core::task::Context<'_>))->Result<Outcome,hibana::EndpointError>{{let _=&db;let(label,receive,cx)=input;match(label,receive){{{arms}_=>panic!(\"unknown scoped admission selector\")}}}}}}"
                );
            }
        }
        code
    }

    fn endpoint(&self, role: u8, names: &[String; 4]) -> String {
        if role == 3 && mobile(self.source) {
            format!("{}.as_mut().expect(\"missing Endpoint value\")", names[3])
        } else {
            names[usize::from(role)].clone()
        }
    }
    fn scope_probe(
        &self,
        source: &Protocol,
        part: &Protocol,
        role: u8,
        actor: u8,
        endpoint: &str,
    ) -> String {
        let Protocol::Par(a, b) = source else {
            unreachable!()
        };
        let group = (
            super::scope_probes::first_label(a),
            super::scope_probes::first_label(b),
        );
        let mut arms = String::new();
        for (message, receive) in super::scope_probes::outside_messages(self.source, part, role) {
            let Protocol::Message { event: label, .. } = message else {
                unreachable!()
            };
            let call = if receive {
                format!("{endpoint}.recv::<M{label}>()")
            } else {
                format!("{endpoint}.send::<M{label}>(&())")
            };
            arms += &format!(
                "({label},{receive})=>{{let mut future=core::pin::pin!({call});future.as_mut().poll(cx)}},"
            );
        }
        if arms.is_empty() {
            return String::new();
        }
        format!(
            "if let Some(probe @ LocalProbe::Borrow{{group:({}, {}),part:{},occurrence,actor:{actor},role:{role},label,receive}})=probe && occurrence==script.first_occurrence(){{let result=core::future::poll_fn(|cx|core::task::Poll::Ready(match (label,receive){{{arms}_=>panic!(\"unknown borrowed-scope probe\")}})).await;match result{{core::task::Poll::Ready(Err(error))=>{{observed.scope_rejections.borrow_mut().push(probe);return Err(error.into())}},other=>panic!(\"operation outside borrowed scope was not rejected: {{probe:?}} {{other:?}}\")}}}}",
            group.0,
            group.1,
            super::scope_probes::first_label(part)
        )
    }

    fn operation(
        &self,
        (message, target_scope): (&Protocol, &Protocol),
        actor: u8,
        names: &[String; 4],
        receive: bool,
    ) -> String {
        let Protocol::Message {
            from,
            to,
            event: label,
            owned,
            ..
        } = message
        else {
            unreachable!()
        };
        let role = if receive { *to } else { *from };
        let resource = role == 3 && mobile(self.source);
        if (resource && actor == 3) || (!resource && role != actor) {
            return String::new();
        }
        let endpoint = if resource {
            "resource".to_owned()
        } else {
            self.endpoint(role, names)
        };
        let call = if receive {
            format!("{endpoint}.recv::<M{label}>()")
        } else {
            format!(
                "{endpoint}.send::<M{label}>({})",
                if *owned {
                    "db.take().expect(\"missing Endpoint to send\")"
                } else {
                    "&()"
                }
            )
        };
        let observed = format!(
            "record_pending({call},observed,ObservedCall::Message({actor},observed_role,{label},{receive})).await?"
        );
        let call = if receive && *owned {
            format!("assert!(db.is_none(),\"duplicate Endpoint value\");db=Some({observed});")
        } else {
            format!("{observed};")
        };
        let endpoint_ref = if resource {
            endpoint.clone()
        } else {
            format!("&{endpoint}")
        };
        let observe = format!("observation_role({endpoint_ref})");
        let mut causal = String::new();
        if receive {
            let arms = self.causal.iter().filter(|(before, _, receiver)| before == label && *receiver == role).map(|(_, target, _)| {
                format!("{target}=>{{let mut future=core::pin::pin!({endpoint}.send::<M{target}>(&()));future.as_mut().poll(cx)}},")
            }).collect::<String>();
            if !arms.is_empty() {
                causal = format!(
                    "if let Some(probe @ LocalProbe::BeforeReceive{{before:{label},occurrence,actor:{actor},role:{role},label}})=probe && occurrence==script.first_occurrence(){{let result=core::future::poll_fn(|cx|core::task::Poll::Ready(match label{{{arms}_=>panic!(\"unknown causal probe\")}})).await;match result{{core::task::Poll::Ready(Err(error))=>{{observed.scope_rejections.borrow_mut().push(probe);return Err(error.into())}},other=>panic!(\"send before required receive was not rejected: script={{script_index}} reverse={{reverse_poll}} {{probe:?}} {{other:?}} prefix={{:?}}\",observed.completed.borrow())}}}}"
                );
            }
        }
        let queries = super::scope_probes::public_queries(
            self.source,
            role,
            (*owned || payload_in_scope(self.source, target_scope)).then_some(3),
        );
        let query_payload =
            if resource || !mobile(self.source) || !payload_in_scope(self.source, target_scope) {
                "&mut None"
            } else {
                "&mut db"
            };
        let admission = |after| {
            if queries.is_empty() {
                return String::new();
            }
            format!(
                "if let Some(probe @ LocalProbe::Admission{{at:{label},at_receive:{receive},after:{after},occurrence,actor:{actor},role:{role},label,receive,choice}})=probe && occurrence==script.first_occurrence(){{let prefix_records=captured.borrow().len();let prefix_borrows=observed.borrows.borrow().len();let prefix_calls=observed.completed.borrow().len();/* SAFETY: this live target uses InMemoryTransport, between executor polls. */let cut=unsafe{{ObservedEndpoint::operation_cut({endpoint_ref})}};assert_eq!(cut.actor.actor,{actor});assert_eq!(cut.target.view,{role});assert!(cut.actor.fault.is_none(),\"baseline query cut is already faulted\");let input=query_inputs.get(choice).expect(\"unplanned resolver input\");for(id,decision)in decisions.iter().enumerate(){{decision.set(std::collections::VecDeque::from([input.contains(&(id as u16))]));}}let result=core::future::poll_fn(|cx|core::task::Poll::Ready({endpoint}.query({query_payload},(label,receive,cx)))).await;let(outcome,error_utf8)=match result{{Ok(outcome)=>(outcome,Vec::new()),Err(error)=>{{if error.is_unavailable(){{/* SAFETY: the query Future has ended; the same actual target remains live. */assert_eq!(unsafe{{ObservedEndpoint::operation_cut({endpoint_ref})}},cut,\"a pre-I/O rejection changed its actual scoped cut\");assert_eq!(captured.borrow().len(),prefix_records,\"a pre-I/O rejection emitted an observation\");}}(Outcome::Rejected,format!(\"{{error:?}}\").into_bytes())}}}};assert!(observed.admission.replace(Some(MeasuredAdmission{{probe,prefix_records,prefix_borrows,prefix_calls,outcome,error_utf8,cut}})).is_none());return Err(TrialError::Measured);}}"
            )
        };
        let preview = if receive
            && !*owned
            && from != to
            && follows_roll_in_choice(self.source, *label)
        {
            format!(
                "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{label})).await?;drop(branch);"
            )
        } else {
            String::new()
        };
        let code = format!(
            "hibana_runtime::yield_now().await;let observed_role={observe};{}{causal}{preview}{call}{}",
            admission(false),
            admission(true)
        );
        if resource {
            format!("if let Some(resource)={}.as_mut(){{{code}}}", names[3])
        } else {
            code
        }
    }

    fn emit(
        &self,
        source: &Protocol,
        actor: u8,
        names: &[String; 4],
        primed: Option<u8>,
    ) -> String {
        let primed = primed.filter(|&entry| contains(source, entry));
        match source {
            Protocol::Message {
                from,
                to,
                event: label,
                ..
            } => {
                let mut code = String::new();
                if from == to {
                    if primed.is_none() {
                        code += &self.operation((source, source), actor, names, false);
                    }
                } else {
                    if primed != Some(*label) {
                        code += &self.operation((source, source), actor, names, false);
                    }
                    code += &self.operation((source, source), actor, names, true);
                }
                if code.is_empty() {
                    code
                } else {
                    format!("assert!(matches!(script,Script::Message({label},_)));{code}")
                }
            }
            Protocol::Seq(a, b) => {
                let a = self.emit(a, actor, names, primed);
                let b = self.emit(b, actor, names, primed);
                if a.is_empty() && b.is_empty() {
                    return String::new();
                }
                let bind = |name: &str, body: &str| {
                    if body.is_empty() {
                        String::new()
                    } else {
                        format!("{{let script={name};{body}}}")
                    }
                };
                format!(
                    "{{let (_left,_right)=match script{{Script::Seq(a,b)=>(a.as_ref(),b.as_ref()),_=>panic!(\"expected sequential script\")}};{}{}}}",
                    bind("_left", &a),
                    bind("_right", &b)
                )
            }
            Protocol::Roll(body) => {
                let first = self.emit(body, actor, names, primed);
                if primed.is_some() {
                    let second = self.emit(body, actor, names, None);
                    if first.is_empty() && second.is_empty() {
                        return String::new();
                    }
                    let first = if first.is_empty() {
                        String::new()
                    } else {
                        format!("{{let script=_first;{first}}}")
                    };
                    format!(
                        "{{let [_first,_second]=match script{{Script::Roll(visits)=>[visits[0].as_ref(),visits[1].as_ref()],_=>panic!(\"expected repeated script\")}};{first}{{let script=_second;{second}}}}}"
                    )
                } else if first.is_empty() {
                    String::new()
                } else {
                    format!(
                        "for visit in match script{{Script::Roll(visits)=>visits,_=>panic!(\"expected repeated script\")}}{{let script=visit.as_ref();{first}}}"
                    )
                }
            }
            Protocol::Route(..) | Protocol::Resolved(..) => {
                if mobile(self.source) {
                    if actor == 3 {
                        return String::new();
                    }
                    let incoming = self
                        .route_owners
                        .iter()
                        .find_map(|(node, owners)| (*node == source).then_some(*owners))
                        .expect("missing source ownership at choice");
                    if incoming == 1 << actor {
                        return self.emit_route(source, actor, actor, names, primed);
                    }
                    if incoming & (1 << actor) == 0 {
                        return self.emit_route(source, actor, u8::MAX, names, primed);
                    }
                    let available = self.emit_route(source, actor, actor, names, primed);
                    let absent = self.emit_route(source, actor, u8::MAX, names, primed);
                    if available == absent {
                        available
                    } else {
                        format!("if {}.is_some(){{{available}}}else{{{absent}}}", names[3])
                    }
                } else {
                    self.emit_route(source, actor, 3, names, primed)
                }
            }
            Protocol::Par(..) => {
                let (declarations, checks, body, returns) =
                    self.parallel_code(source, actor, names, primed);
                if body.is_empty() {
                    body
                } else {
                    format!(
                        "{{{declarations}{checks}{}{body};}}",
                        if returns { "db=" } else { "" }
                    )
                }
            }
        }
    }
    fn emit_route(
        &self,
        source: &Protocol,
        actor: u8,
        before: u8,
        names: &[String; 4],
        primed: Option<u8>,
    ) -> String {
        let (a, b) = match source {
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => (a, b),
            _ => unreachable!(),
        };
        let select = "let (script,right)=match script{Script::Branch(body,right)=>(body.as_ref(),*right),_=>panic!(\"expected branch script\")};";
        if let Some(entry) = primed {
            let label = entry;
            let right = contains(b, label);
            let body = self.emit(if right { b } else { a }, actor, names, primed);
            if body.is_empty() {
                return body;
            }
            return format!("{{{select}assert_eq!(right,{right});{body}}}");
        }
        let first = first_local(source, actor, before);
        if first.is_empty() {
            return String::new();
        }
        let mut entry = Vec::new();
        heads(source, &mut entry);
        let controller_role = match entry[0] {
            Protocol::Message { from, .. } => *from,
            _ => unreachable!(),
        };
        let controller = if controller_role == 3 {
            before
        } else {
            controller_role
        };
        let preview_for = |view: u8, entry: u8, allowed: &str| {
            let endpoint = self.endpoint(view, names);
            format!(
                "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;let label=branch.label();drop(branch);let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;assert_eq!(branch.label(),label);drop(branch);assert!(({allowed}).contains(&label),\"unexpected branch: actor={actor} entry={entry} expected={allowed} label={{label}} script={{script_index}} reverse={{reverse_poll}} prefix={{:?}}\",observed.completed.borrow());"
            )
        };
        let arm = |body: &Protocol| {
            if actor == controller {
                let mut entries = Vec::new();
                heads(body, &mut entries);
                let mut arms = String::new();
                let mut previewed = std::collections::BTreeSet::new();
                for message in &entries {
                    let Protocol::Message { event: label, .. } = message else {
                        unreachable!()
                    };
                    if matches!(source, Protocol::Route(..)) || message_selected_head(body, *label)
                    {
                        let send = self.operation((message, source), actor, names, false);
                        let rest = self.emit(body, actor, names, Some(*label));
                        arms += &format!(
                            "{label}=>{{{{let script=script.first_scheduled_message();assert!(matches!(script,Script::Message({label},_)));{send}}}{rest}}},"
                        );
                    } else if previewed.insert(self.paths[label].clone()) {
                        let alternatives = entries
                            .iter()
                            .filter_map(|message| match message {
                                Protocol::Message { event: other, .. }
                                    if !message_selected_head(body, *other)
                                        && self.paths[other] == self.paths[label] =>
                                {
                                    Some(other.to_string())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("|");
                        // A completed roll and its suffix can both be enabled.
                        // The controller's script selects the source arm; offer
                        // need not preview that arm's first operation. Preserve
                        // the preview while driving the complete typed body.
                        let endpoint = self.endpoint(controller_role, names);
                        let preview = format!(
                            "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{label})).await?;let label=branch.label();drop(branch);let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{label})).await?;assert_eq!(branch.label(),label);drop(branch);"
                        );
                        let rest = self.emit(body, actor, names, None);
                        arms += &format!("{alternatives}=>{{{preview}{rest}}},");
                    }
                }
                format!("match script.trace()[0].0{{{arms}_=>panic!(\"unknown branch entry\")}}")
            } else {
                self.emit(body, actor, names, None)
            }
        };
        if actor == controller {
            let left = arm(a);
            let right = arm(b);
            return format!("{{{select}if right{{{right}}}else{{{left}}}}}");
        }
        // An identical public operation does not reveal the choice. Execute
        // it before offering the remaining alternatives. Script selection
        // chooses occurrence tags only: both emitted calls have the same
        // logical roles, message contract, direction and payload kind.
        let mut left_source = *a.clone();
        let mut right_source = *b.clone();
        let mut before = before;
        let mut common = String::new();
        while let (Some((a, at)), Some((b, bt))) =
            (split_first(&left_source), split_first(&right_source))
        {
            if !same_message(a, b) || at.is_some() != bt.is_some() {
                break;
            }
            let left = self.emit(a, actor, names, None);
            let right = self.emit(b, actor, names, None);
            common += &if left.is_empty() && right.is_empty() {
                "let (_,continuation)=script.split_first_message();".into()
            } else {
                format!(
                    "let (entry,continuation)=script.split_first_message();{{let script=entry;if right{{{right}}}else{{{left}}}}}"
                )
            };
            let (Some(at), Some(bt)) = (at, bt) else {
                return format!("{{{select}{common}assert!(continuation.is_none());}}");
            };
            common += "let continuation=continuation.expect(\"shared entry requires a continuation\");let script=&continuation;";
            if let Protocol::Message {
                to, owned: true, ..
            } = a
            {
                before = *to;
            }
            left_source = at;
            right_source = bt;
        }
        let left = self.emit(&left_source, actor, names, None);
        let right = self.emit(&right_source, actor, names, None);
        let left_entries = first_local(&left_source, actor, before);
        let right_entries = first_local(&right_source, actor, before);
        if left_entries.is_empty() && right_entries.is_empty() {
            return format!("{{{select}{common}}}");
        }
        let left_done = left_entries.is_empty();
        let right_done = right_entries.is_empty();
        let mut views = std::collections::BTreeMap::new();
        for (view, label) in left_entries.iter().chain(&right_entries) {
            views.entry(*view).or_insert(*label);
        }
        assert!(!views.is_empty());
        let preview = if views.len() == 1 {
            let (&view, &entry) = views.first_key_value().unwrap();
            let allowed = left_entries
                .iter()
                .chain(&right_entries)
                .map(|(_, event)| self.source.contract_for(*event).to_string())
                .collect::<Vec<_>>()
                .join(",");
            preview_for(view, entry, &format!("&[{allowed}][..]"))
        } else {
            let mut offers = Vec::new();
            let mut retake = String::new();
            for (view, entry) in views {
                let endpoint = self.endpoint(view, names);
                offers.push(format!(
                    "async{{let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;let label=branch.label();drop(branch);Ok::<_,TrialError>(label)}}"
                ));
                let labels = left_entries
                    .iter()
                    .chain(&right_entries)
                    .filter(|(role, _)| *role == view)
                    .map(|(_, label)| *label)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .map(|event| self.source.contract_for(event).to_string())
                    .collect::<Vec<_>>()
                    .join("|");
                retake += &format!(
                    "{labels}=>{{let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;assert_eq!(branch.label(),label);drop(branch);}},"
                );
            }
            let offers = offers
                .into_iter()
                .reduce(|left, right| format!("hibana_runtime::race({left},{right})"))
                .expect("a choice has an incoming view");
            format!(
                "let label={offers}.await?;match label{{{retake}_=>panic!(\"unknown incoming view\")}}"
            )
        };
        let mut branches = String::new();
        let left_labels = left_entries
            .into_iter()
            .map(|(_, event)| self.source.contract_for(event))
            .collect::<std::collections::BTreeSet<_>>();
        let right_labels = right_entries
            .into_iter()
            .map(|(_, event)| self.source.contract_for(event))
            .collect::<std::collections::BTreeSet<_>>();
        for label in left_labels.union(&right_labels) {
            let body = match (left_labels.contains(label), right_labels.contains(label)) {
                // A reused public contract does not identify a source arm.
                // Exercise the planned word compatible with this preview;
                // actual occurrence/choice and every admission are checked
                // independently from the captured records by the source oracle.
                (true, true) => format!("if right{{{right}}}else{{{left}}}"),
                (true, false) => format!(
                    "assert_eq!(right,false,\"incoming preview chose another source arm: actor={actor} label={{label}} script={{script_index}} reverse={{reverse_poll}} prefix={{:?}}\",observed.completed.borrow());{left}"
                ),
                (false, true) => format!(
                    "assert_eq!(right,true,\"incoming preview chose another source arm: actor={actor} label={{label}} script={{script_index}} reverse={{reverse_poll}} prefix={{:?}}\",observed.completed.borrow());{right}"
                ),
                _ => unreachable!(),
            };
            branches += &format!("{label}=>{{{body}}},");
        }
        let remaining =
            format!("{preview}match label{{{branches}_=>panic!(\"unknown received branch\")}}");
        // A prescribed finite source word needs no further operation from
        // this actor on an empty residual path. Offering there can observe a
        // later visit, or wait forever at the end of this word. Keep both
        // previews on every nonempty path; the independent source checker
        // still checks the actual common occurrence and complete word.
        let remaining = if left_done {
            format!("if right{{{remaining}}}else{{{left}}}")
        } else if right_done {
            format!("if right{{{right}}}else{{{remaining}}}")
        } else {
            remaining
        };
        format!("{{{select}{common}{remaining}}}")
    }

    fn parallel_code(
        &self,
        source: &Protocol,
        actor: u8,
        names: &[String; 4],
        primed: Option<u8>,
    ) -> (String, String, String, bool) {
        let mut parts = Vec::new();
        parallel_parts_for_actor(source, actor, &mut parts);
        let mut names = names.clone();
        let mut declarations = String::new();
        // A received owned endpoint starts at the whole view. Enter enclosing
        // mobile parallel groups before borrowing this nested group. The
        // generated code keeps these borrows alive until the nested work ends.
        if names[3] == "db"
            && mobile(self.source)
            && !mobile(source)
            && actor < 3
            && present(source, 3, None)
        {
            for (depth, parent) in parallel_ancestors(self.source, source)
                .expect("parallel source must belong to the global")
                .into_iter()
                .enumerate()
            {
                let mut parent_parts = Vec::new();
                parallel_parts(parent, &mut parent_parts);
                assert_eq!(
                    parent_parts
                        .iter()
                        .filter(|part| present(part, 3, None))
                        .count(),
                    1,
                    "a mobile group cannot use its resource in another child"
                );
                let name = format!("resource_parent_{depth}");
                declarations += &format!(
                    "let [mut {name}]=if let Some(resource)={}.as_mut(){{let [part]=resource.par()?;[Some(part)]}}else{{[None]}};",
                    names[3]
                );
                let part = parent_parts
                    .into_iter()
                    .find(|part| present(part, 3, None))
                    .unwrap();
                declarations += &format!(
                    "if {name}.is_some(){{{}}}",
                    borrow_observation(parent, part, actor, 3)
                );
                names[3] = name;
            }
        }
        let mut part_names = vec![names.clone(); parts.len()];
        let mut scope_checks = String::new();
        for role in 0..4 {
            let optional = role == 3 && mobile(self.source);
            if if optional {
                actor == 3 || mobile(source)
            } else {
                role != usize::from(actor)
            } {
                continue;
            }
            let indices = (0..parts.len())
                .filter(|i| present(parts[*i], role as u8, None))
                .collect::<Vec<_>>();
            if indices.is_empty() {
                continue;
            }
            if !optional
                && primed.is_none()
                && indices.len() > 1
                && parts.iter().all(|part| {
                    matches!(part,
                    Protocol::Message { from, owned: false, .. } if usize::from(*from) == role)
                })
                && follows_roll_in_choice(self.source, bounds(source).0 as u8)
            {
                // Both outgoing children follow the completed input/retry
                // prefix. Retain a real offer before borrowing: it may name
                // the roll's alternative rather than either child.
                let endpoint = &names[role];
                let entry = bounds(source).0;
                declarations += &format!(
                    "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;drop(branch);"
                );
            }
            let mut inner = Vec::new();
            let bindings = indices
                .iter()
                .map(|i| {
                    let used = present(parts[*i], role as u8, primed);
                    let name = format!("{}p{role}_{i}", if used { "" } else { "_" });
                    part_names[*i][role] = name.clone();
                    inner.push(name.clone());
                    let observation = borrow_observation(source, parts[*i], actor, role);
                    scope_checks += &if optional {
                        format!("if {name}.is_some(){{{observation}}}")
                    } else {
                        observation
                    };
                    if used {
                        let endpoint = self.endpoint(role as u8, &part_names[*i]);
                        let check =
                            self.scope_probe(source, parts[*i], role as u8, actor, &endpoint);
                        scope_checks += &if optional {
                            format!("if {name}.is_some(){{{check}}}")
                        } else {
                            check
                        };
                    }
                    format!("{}{name}", if used { "mut " } else { "" })
                })
                .collect::<Vec<_>>()
                .join(",");
            if optional {
                let values = inner
                    .iter()
                    .map(|name| format!("Some({name})"))
                    .collect::<Vec<_>>()
                    .join(",");
                let empty = vec!["None"; indices.len()].join(",");
                declarations += &format!(
                    "let [{bindings}]=if let Some(resource)={}.as_mut(){{let [{}]=resource.par()?;[{values}]}}else{{[{empty}]}};",
                    names[role],
                    inner.join(",")
                );
            } else {
                declarations += &format!("let [{bindings}]={}.par()?;", names[role]);
            }
        }
        fn schedule(
            e: &Emitter<'_>,
            source: &Protocol,
            actor: u8,
            parts: &[&Protocol],
            names: &[[String; 4]],
            part: &mut usize,
            primed: Option<u8>,
        ) -> (String, bool) {
            if let Protocol::Par(a, b) = source {
                let (left, returns_left) = schedule(e, a, actor, parts, names, part, primed);
                let (right, returns_right) = schedule(e, b, actor, parts, names, part, primed);
                assert!(!returns_left || !returns_right);
                if left.is_empty() && right.is_empty() {
                    return (String::new(), false);
                }
                let bind = |plan: &str, body: &str| {
                    if body.is_empty() {
                        "async{Ok::<_,TrialError>(())}".to_owned()
                    } else {
                        format!(
                            "async{{let script={plan};let result={body};Ok::<_,TrialError>(result)}}"
                        )
                    }
                };
                let result = if returns_left {
                    "_a"
                } else if returns_right {
                    "_b"
                } else {
                    "()"
                };
                (
                    format!(
                        "{{let (_left,_right,reverse)=match script{{Script::Par(a,b,reverse)=>(a.as_ref(),b.as_ref(),*reverse),_=>panic!(\"expected parallel script\")}};let left_future={};let right_future={};let (_a,_b)=if reverse ^ reverse_poll{{let (b,a)=async{{async{{right_future.await}};async{{left_future.await}};}}.await?;(a,b)}}else{{async{{async{{left_future.await}};async{{right_future.await}};}}.await?}};{result}}}",
                        bind("_left", &left),
                        bind("_right", &right)
                    ),
                    returns_left || returns_right,
                )
            } else if let Protocol::Seq(first, _) = source
                && parts[*part] != source
            {
                let (body, returns) = schedule(e, first, actor, parts, names, part, primed);
                (
                    format!(
                        "{{let script=match script{{Script::Seq(first,_)=>first.as_ref(),_=>panic!(\"expected sequential script\")}};{body}}}"
                    ),
                    returns,
                )
            } else {
                let names = &names[*part];
                *part += 1;
                let body = e.emit(source, actor, names, primed);
                let returns = mobile(source) && actor < 3;
                if body.is_empty() && !returns {
                    return (body, false);
                }
                let init = if returns {
                    format!(
                        "let {}db=db;",
                        if borrows_resource_mutably(source, actor) {
                            "mut "
                        } else {
                            ""
                        }
                    )
                } else {
                    String::new()
                };
                (
                    format!("{{{init}{body}{}}}", if returns { "db" } else { "()" }),
                    returns,
                )
            }
        }
        let (body, returns) = schedule(self, source, actor, &parts, &part_names, &mut 0, primed);
        (declarations, scope_checks, body, returns)
    }
}

pub(super) fn runs(plans: &super::plans::Plans<'_>, owner: u8, case: usize) -> String {
    let source = plans.source();
    let script_count = plans.len();
    let mut paths = std::collections::BTreeMap::new();
    source.resolver_choices(&mut Vec::new(), &mut paths);
    let mut choices = Vec::new();
    if mobile(source) {
        route_owners(source, 1 << owner, &mut choices);
    }
    let e = Emitter {
        source,
        paths,
        causal: super::causality::pairs(source),
        route_owners: choices,
    };
    let resolvers = e
        .paths
        .values()
        .flatten()
        .map(|(id, _)| *id)
        .collect::<std::collections::BTreeSet<_>>();
    let query_inputs = super::decisions::patterns(source, resolvers.len().try_into().unwrap());
    let initial_inputs = resolver_input_code(source);
    let input_array = query_inputs
        .iter()
        .map(|input| format!("&{input:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut code = format!(
        r#"
    if let Some(program)=programs.p0.as_ref(){{write_operation_image(program,{case});}}
    if let Some(program)=programs.p1.as_ref(){{write_operation_image(program,{case});}}
    if let Some(program)=programs.p2.as_ref(){{write_operation_image(program,{case});}}
    if let Some(program)=programs.p3.as_ref(){{write_operation_image(program,{case});}}
    let query_inputs:&[&[u16]]=&[{input_array}];
    let mut planned_words=std::collections::BTreeSet::new();
    let script_range=selected_script_range({script_count});
    let scripts=std::io::BufRead::lines(std::io::BufReader::new(std::fs::File::open(std::path::Path::new(PROTOCOL_SOURCE_DIR).join("scripts/{case}.jsonl")).expect("missing script plans")));
    use std::io::Write as _;
    let scope_root=std::path::Path::new(PROTOCOL_SOURCE_DIR);
    let mut scope_plans=ScopePlans::new(std::io::BufReader::new(std::fs::File::open(scope_root.join("scope-plans/{case}.jsonl")).expect("missing scope plans")),{case},{});
    let recorded=observation_root();
    std::fs::create_dir_all(recorded.join("scope-observations")).unwrap();
    let mut scope_observations=std::io::BufWriter::new(std::fs::File::create(recorded.join("scope-observations/{case}.jsonl")).unwrap());
    writeln!(scope_observations,"{{{{\"name\":{case},\"scripts\":{}}}}}").unwrap();
    std::fs::create_dir_all(recorded.join("captures")).unwrap();
    let mut captures=std::io::BufWriter::new(std::fs::File::create(recorded.join("captures/{case}.jsonl")).unwrap());
    writeln!(captures,"{{{{\"name\":{case},\"scripts\":{script_count},\"first\":{{}},\"count\":{{}}}}}}",script_range.start,script_range.len()).unwrap();
    std::fs::create_dir_all(recorded.join("admission-observations")).unwrap();
    let mut admissions=std::io::BufWriter::new(std::fs::File::create(recorded.join("admission-observations/{case}.jsonl")).unwrap());
    writeln!(admissions,"{{{{\"name\":{case},\"scripts\":{script_count},\"first\":{{}},\"count\":{{}},\"sourceConformanceChecked\":false,\"resolverInputs\":{{query_inputs:?}}}}}}",script_range.start,script_range.len()).unwrap();
    std::fs::create_dir_all(recorded.join("operation-states")).unwrap();
    let mut states=std::io::BufWriter::new(std::fs::File::create(recorded.join("operation-states/{case}.jsonl")).unwrap());
    let mut state_ids=std::collections::BTreeMap::<String,usize>::new();
    let mut projection=std::io::BufWriter::new(std::fs::File::create(recorded.join("captures/{case}.projection.json")).unwrap());
    let snapshot=if let Some(program)=programs.p0.as_ref(){{hibana::runtime::tap::projected(program).collect::<Vec<_>>()}}else if let Some(program)=programs.p1.as_ref(){{hibana::runtime::tap::projected(program).collect::<Vec<_>>()}}else if let Some(program)=programs.p2.as_ref(){{hibana::runtime::tap::projected(program).collect::<Vec<_>>()}}else{{hibana::runtime::tap::projected(programs.p3.as_ref().expect("missing source program")).collect::<Vec<_>>()}};
    write!(projection,"[").unwrap();for(index,event)in snapshot.iter().enumerate(){{if index!=0{{write!(projection,",").unwrap();}}write!(projection,"[{{}},{{}},{{}},{{}},{{}}]",event.ts(),event.id(),event.causal_key(),event.arg0(),event.arg1()).unwrap();}}writeln!(projection,"]").unwrap();projection.flush().unwrap();
    let mut checked_scripts=0usize;
    for (script_index,line) in scripts.enumerate() {{ let decoded_script=Script::from_json(&line.expect("unreadable script plan")).expect("invalid script plan");let script=&decoded_script;let probes=scope_plans.next(script_index);
    if script_range.start==0{{planned_words.insert(script.line());}}
    if !script_range.contains(&script_index){{continue;}}
    for reverse_poll in [false,true] {{
    write!(scope_observations,"{{{{\"script\":{{script_index}},\"reversePoll\":{{reverse_poll}}").unwrap();
    let mut scoped_rejections=Vec::new();
    let mut baseline_events=Vec::new();let mut baseline_borrows=Vec::new();let mut baseline_calls=Vec::new();
    let expected_admissions=probes.iter().filter(|probe|matches!(probe,LocalProbe::Admission{{..}})).count();let mut measured_admissions=0usize;
    writeln!(admissions,"{{{{\"kind\":\"inventory\",\"script\":{{script_index}},\"reversePoll\":{{reverse_poll}},\"queries\":{{expected_admissions}}}}}}").unwrap();
    for probe in core::iter::once(None).chain(probes.iter().copied().map(Some)) {{
      let _=reverse_poll;
      let mut decision_inputs:Vec<std::collections::VecDeque<bool>>=(0..{}).map(|_|std::collections::VecDeque::new()).collect();{initial_inputs}
      let decisions:Vec<_>=decision_inputs.into_iter().map(std::cell::Cell::new).collect();
      let sid=SessionId::new(2);
      let captured=std::cell::RefCell::new(Vec::new());
      let capture=|producer:usize,event:hibana::runtime::tap::TapEvent|captured.borrow_mut().push((producer,event));
      let capture0=|event|capture(0,event);let capture1=|event|capture(1,event);
      let capture2=|event|capture(2,event);let capture3=|event|capture(3,event);
      // SAFETY: this execution owns its resource and its only initial issuer.
      let network=InMemoryTransport::new(programs.issuance.iter().map(|&(role,_)|((sid,role),unsafe{{EndpointBinding::new()}})));
      let [mut slab0,mut slab1,mut slab2,mut slab3]=[vec![0;128*1024],vec![0;128*1024],vec![0;128*1024],vec![0;128*1024]];
      let mut s0=SessionKitStorage::uninit();let mut s1=SessionKitStorage::uninit();let mut s2=SessionKitStorage::uninit();let mut s3=SessionKitStorage::uninit();
      let rv0=s0.init().rendezvous(&mut slab0,network.clone()).unwrap();
      let rv1=s1.init().rendezvous(&mut slab1,network.clone()).unwrap();
      let rv2=s2.init().rendezvous(&mut slab2,network.clone()).unwrap();
      let rv3=s3.init().rendezvous(&mut slab3,network.clone()).unwrap();
      if probe.is_none() || matches!(probe,Some(LocalProbe::Admission{{..}})){{rv0.observe(&capture0);rv1.observe(&capture1);rv2.observe(&capture2);rv3.observe(&capture3);}}
    "#,
        script_count,
        script_count,
        resolvers.len()
    );
    code.insert_str(0, &e.query_dispatch());
    for id in resolvers {
        let choose = format!(
            "|d|{{let mut choices=d[{id}].take();let right=*choices.front().expect(\"unplanned resolver input\");if choices.len()>1{{choices.pop_front();}}d[{id}].set(choices);Ok(if right{{DecisionArm::Right}}else{{DecisionArm::Left}})}}"
        );
        code += &format!("match programs.resolver_roles[{id}]{{");
        for actor in 0..3 {
            code += &format!(
                "{actor}=>rv{actor}.set_resolver(programs.p{actor}.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&decisions,{choose})).unwrap(),"
            );
        }
        code += "3=>{";
        for actor in 0..4 {
            code += &format!(
                "if {}{{rv{actor}.set_resolver(programs.p3.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&decisions,{choose})).unwrap();}}",
                if actor == 3 {
                    "programs.issuance.is_empty()"
                } else {
                    "!programs.issuance.is_empty()"
                }
            );
        }
        code += "},_=>panic!(\"unknown choosing role\")};";
    }
    for actor in 0..3 {
        if present(source, actor, None) {
            code += &format!(
                "let mut e{actor}=rv{actor}.enter(sid,programs.p{actor}.as_ref().unwrap()).unwrap();"
            );
        } else {
            code += &format!("let _=&rv{actor};");
        }
    }
    if mobile(source) {
        code +=
            &format!("let initial_db=rv{owner}.enter(sid,programs.p3.as_ref().unwrap()).unwrap();");
    }
    if !mobile(source) && present(source, 3, None) {
        code += "let mut e3=rv3.enter(sid,programs.p3.as_ref().unwrap()).unwrap();";
    } else {
        code += "let _=&rv3;";
    }
    code += "let observed=&PendingCalls::default();";
    code += "let result=async{";
    for actor in 0..4 {
        let db = if mobile(source) && actor < 3 {
            format!(
                "let {}db:Option<Endpoint<'_,3>>={};",
                if borrows_resource_mutably(source, actor) {
                    "mut "
                } else {
                    ""
                },
                if actor == owner {
                    "Some(initial_db)"
                } else {
                    "None"
                }
            )
        } else {
            String::new()
        };
        let body = if mobile(source) && actor == 3 {
            String::new()
        } else {
            e.emit(
                source,
                actor,
                &[
                    "e0".into(),
                    "e1".into(),
                    "e2".into(),
                    if mobile(source) {
                        "db".into()
                    } else {
                        "e3".into()
                    },
                ],
                None,
            )
        };
        // Exercise the same fresh receive with and without already queued input.
        // Alternating only the parallel branches left the initial sender late
        // in both schedules, even when the source contained no parallel node.
        let yield_first = if actor == owner {
            "!reverse_poll"
        } else {
            "reverse_poll"
        };
        code += &format!(
            "async{{{db}if {yield_first}{{hibana_runtime::yield_now().await;}}{body}{}Ok::<_,TrialError>(())}};",
            if mobile(source) && actor < 3 {
                "drop(db);"
            } else {
                ""
            }
        );
    }
    code += r#"}.await;
    if let Some(probe @ LocalProbe::Admission{..})=probe{
      assert!(matches!(result,Err(TrialError::Measured)),"scoped admission did not reach its actual handle: {probe:?} {result:?}");
      assert!(observed.calls.borrow().is_empty(),"scoped trial left a pending call after cancellation");
      let measured=observed.admission.take().expect("missing scoped admission result");assert_eq!(measured.probe,probe);
      let events=captured.borrow();let borrows=observed.borrows.borrow();let calls=observed.completed.borrow();
      assert!(measured.prefix_records<=baseline_events.len() && events[..measured.prefix_records]==baseline_events[..measured.prefix_records],"scoped admission changed the preceding actual capture: {probe:?}");
      assert!(measured.prefix_borrows<=baseline_borrows.len() && borrows[..measured.prefix_borrows]==baseline_borrows[..measured.prefix_borrows],"scoped admission changed the preceding borrow boundaries");
      assert!(measured.prefix_calls<=baseline_calls.len() && calls[..measured.prefix_calls]==baseline_calls[..measured.prefix_calls],"scoped admission changed the preceding completed calls");
      let outcome=match measured.outcome{Outcome::Accepted=>"completed",Outcome::Pending=>"pending",Outcome::Rejected=>"rejected",Outcome::Unavailable=>panic!("a real scoped call cannot be unavailable")};
      write!(admissions,"{{\"script\":{script_index},\"reversePoll\":{reverse_poll},\"probe\":{},\"prefixRecords\":{},\"prefixBorrows\":{},\"prefixCalls\":{},\"outcome\":\"{outcome}\",\"errorUtf8\":{:?},\"checkpoints\":{:?},",probe.json(),measured.prefix_records,measured.prefix_borrows,measured.prefix_calls,measured.error_utf8,[&rv0,&rv1,&rv2,&rv3].map(|rv|rv.tap().checkpoint().expect("capture ordinal exhausted"))).unwrap();
      let payload=operation_state_json(&measured.cut);
      let state=if let Some(&id)=state_ids.get(&payload){id}else{let id=state_ids.len();writeln!(states,"{{\"id\":{id},{}",&payload[1..]).unwrap();state_ids.insert(payload,id);id};
      write!(admissions,"\"state\":{state},\"suffix\":[").unwrap();
      for(index,(producer,event))in events[measured.prefix_records..].iter().enumerate(){if index!=0{write!(admissions,",").unwrap();}write!(admissions,"[{producer},{},{},{},{},{}]",event.ts(),event.id(),event.causal_key(),event.arg0(),event.arg1()).unwrap();}writeln!(admissions,"]}}").unwrap();measured_admissions+=1;continue;
    }
    if let Some(probe)=probe{assert!(result.is_err(),"local probe did not stop its run");let actual=observed.scope_rejections.borrow();assert_eq!(*actual,[probe],"planned local probe was not reached");scoped_rejections.push(actual[0].json());continue;}
    result.unwrap_or_else(|error|panic!("generated baseline failed: script={script_index}, reverse={reverse_poll}, error={error:?}, completed={:?}",observed.completed.borrow()));
    write!(scope_observations,",\"completed\":[").unwrap();
    for (index,(actor,role,label,receive)) in observed.completed.borrow().iter().enumerate(){if index!=0{write!(scope_observations,",").unwrap();}write!(scope_observations,"[{actor},{role},{label},{receive}]").unwrap();}
    write!(scope_observations,"]").unwrap();
    write!(captures,"{{\"script\":{script_index},\"reversePoll\":{reverse_poll},\"checkpoints\":{:?},\"events\":[",[&rv0,&rv1,&rv2,&rv3].map(|rv|rv.tap().checkpoint().expect("capture ordinal exhausted"))).unwrap();
    for (index,(producer,event)) in captured.borrow().iter().enumerate(){if index!=0{write!(captures,",").unwrap();}write!(captures,"[{producer},{},{},{},{},{}]",event.ts(),event.id(),event.causal_key(),event.arg0(),event.arg1()).unwrap();}
    write!(captures,"],\"borrows\":{:?}",observed.borrows.borrow()).unwrap();
    writeln!(captures,"}}").unwrap();
    baseline_events=captured.borrow().clone();baseline_borrows=observed.borrows.borrow().clone();baseline_calls=observed.completed.borrow().clone();
    assert!(observed.calls.borrow().is_empty());
    "#;
    code += r#"}assert_eq!(measured_admissions,expected_admissions,"missing scoped admission measurements");
    writeln!(scope_observations,",\"probes\":[{}]}}",scoped_rejections.join(",")).unwrap();
    }checked_scripts+=1;}
    assert_eq!(checked_scripts,script_range.len(),"missing assigned script");scope_plans.finish();
    scope_observations.flush().unwrap();captures.flush().unwrap();admissions.flush().unwrap();states.flush().unwrap();
    if script_range.start!=0{return Ok(());}
    "#;
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_tails_erase_only_for_the_view_with_no_later_operation() {
        fn group(source: &Protocol) -> &Protocol {
            match source {
                Protocol::Seq(_, tail) => group(tail),
                Protocol::Par(..) => source,
                _ => panic!("expected the finite catalog's final parallel group"),
            }
        }
        for rank in [33, 34] {
            let source = super::super::contracts::protocol(rank);
            let root = group(&source);
            let sender = if rank == 33 { 0 } else { 1 };
            let mut leaves = Vec::new();
            parallel_parts_for_actor(root, sender, &mut leaves);
            assert_eq!(leaves.len(), 3);
            assert!(
                leaves
                    .iter()
                    .all(|part| matches!(part, Protocol::Message { from, .. } if *from == sender))
            );
            leaves.clear();
            parallel_parts_for_actor(root, 2, &mut leaves);
            assert_eq!(
                leaves.iter().filter(|part| present(part, 2, None)).count(),
                2
            );
            let repeated = Protocol::Roll(Box::new(root.clone()));
            leaves.clear();
            parallel_parts_for_actor(&repeated, sender, &mut leaves);
            assert_eq!(leaves, [&repeated]);
        }
        let mut source = super::super::contracts::protocol(33);
        let Protocol::Par(_, right) = &mut source else {
            unreachable!()
        };
        let Protocol::Seq(_, tail) = right.as_mut() else {
            unreachable!()
        };
        **tail = message(0, 2, false);
        let mut leaves = Vec::new();
        parallel_parts_for_actor(&source, 0, &mut leaves);
        assert_eq!(leaves.len(), 2, "a real local suffix retains its join");
    }

    fn message(from: u8, to: u8, owned: bool) -> Protocol {
        Protocol::Message {
            from,
            to,
            event: 0,
            owned,
            contract: 0,
        }
    }

    #[test]
    fn source_ownership_at_a_choice_preserves_genuine_path_dependence() {
        let source = Protocol::Seq(
            Box::new(message(0, 1, true)),
            Box::new(Protocol::Seq(
                Box::new(Protocol::Route(
                    Box::new(message(1, 0, true)),
                    Box::new(message(1, 2, true)),
                )),
                Box::new(Protocol::Route(
                    Box::new(message(3, 2, false)),
                    Box::new(message(3, 2, false)),
                )),
            )),
        );
        let mut choices = Vec::new();
        assert_eq!(route_owners(&source, 1, &mut choices), 5);
        assert_eq!(
            choices
                .iter()
                .map(|(_, owners)| *owners)
                .collect::<Vec<_>>(),
            [2, 5]
        );
        let ends = source
            .executions()
            .map(|execution| holder_after(&source, &execution, 0))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ends, [0, 2].into());
    }

    #[test]
    fn five_nested_choices_have_one_source_owner_at_each_entry() {
        let generated = super::super::interactions::spine(5, 22409);
        let mut choices = Vec::new();
        assert_eq!(route_owners(&generated.source, 1, &mut choices), 1);
        assert_eq!(choices.len(), 5);
        assert!(choices.iter().all(|(_, owners)| *owners == 2));
    }

    #[test]
    fn shared_prefix_splitting_preserves_parallel_and_repeated_children() {
        let first = message(0, 1, false);
        let parallel = Protocol::Par(
            Box::new(message(1, 2, false)),
            Box::new(message(0, 2, false)),
        );
        let repeated = Protocol::Roll(Box::new(message(2, 0, false)));
        let source = Protocol::Seq(
            Box::new(Protocol::Seq(
                Box::new(first.clone()),
                Box::new(parallel.clone()),
            )),
            Box::new(repeated.clone()),
        );
        let (head, rest) = split_first(&source).unwrap();
        assert!(head == &first);
        assert!(
            rest == Some(Protocol::Seq(
                Box::new(parallel.clone()),
                Box::new(repeated.clone())
            ))
        );
        assert!(split_first(&parallel).is_none());
        assert!(split_first(&repeated).is_none());
    }
}
