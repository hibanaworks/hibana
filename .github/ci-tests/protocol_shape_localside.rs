//! Emit typed localsides from the same immutable syntax as the test input.
use super::{Execution, Protocol};

pub(super) fn contains(source: &Protocol, label: u8) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { label: candidate, .. } if *candidate == label))
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

pub(super) fn present(source: &Protocol, role: u8, primed: Option<u8>) -> bool {
    match source {
        Protocol::Message {
            from, to, label, ..
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

fn parallel_ancestors<'a>(source: &'a Protocol, target: &Protocol) -> Option<Vec<&'a Protocol>> {
    if core::ptr::eq(source, target) {
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

fn first_local(source: &Protocol, actor: u8, holder: u8) -> Vec<(u8, u8)> {
    let physical = |role| if role == 3 { holder } else { role };
    match source {
        Protocol::Message {
            from, to, label, ..
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Consumed(u8),
    LocalPreview(u8),
}

impl Entry {
    fn label(self) -> u8 {
        match self {
            Self::Consumed(label) | Self::LocalPreview(label) => label,
        }
    }

    fn consumed(self) -> Option<u8> {
        match self {
            Self::Consumed(label) => Some(label),
            Self::LocalPreview(_) => None,
        }
    }
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
            let Protocol::Message { label, .. } = message else {
                unreachable!()
            };
            let call = if receive {
                format!("{endpoint}.recv::<M{label}<'_>>()")
            } else {
                format!("{endpoint}.send::<M{label}<'_>>(&())")
            };
            arms += &format!(
                "({label},{receive})=>{{let future={call};tokio::pin!(future);future.as_mut().poll(cx)}},"
            );
        }
        if arms.is_empty() {
            return String::new();
        }
        format!(
            "if let Some(probe @ LocalProbe::Borrow{{group:({}, {}),part:{},occurrence,actor:{actor},role:{role},label,receive}})=probe && occurrence==script.first_occurrence(){{let result=core::future::poll_fn(|cx|core::task::Poll::Ready(match (label,receive){{{arms}_=>panic!(\"unknown borrowed-scope probe\")}})).await;match result{{core::task::Poll::Ready(Err(error))=>{{observed.scope_rejections.borrow_mut().push(probe);return Err(error)}},other=>panic!(\"operation outside borrowed scope was not rejected: {{probe:?}} {{other:?}}\")}}}}",
            group.0,
            group.1,
            super::scope_probes::first_label(part)
        )
    }

    fn operation(
        &self,
        message: &Protocol,
        actor: u8,
        names: &[String; 4],
        receive: bool,
    ) -> String {
        let Protocol::Message {
            from,
            to,
            label,
            owned,
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
            format!("{endpoint}.recv::<M{label}<'_>>()")
        } else {
            format!(
                "{endpoint}.send::<M{label}<'_>>({})",
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
        let observe = if resource {
            format!("observation_role({endpoint})")
        } else {
            format!("observation_role(&{endpoint})")
        };
        let mut causal = String::new();
        if receive {
            let arms = self.causal.iter().filter(|(before, _, receiver)| before == label && *receiver == role).map(|(_, target, _)| {
                format!("{target}=>{{let future={endpoint}.send::<M{target}<'_>>(&());tokio::pin!(future);future.as_mut().poll(cx)}},")
            }).collect::<String>();
            if !arms.is_empty() {
                causal = format!(
                    "if let Some(probe @ LocalProbe::BeforeReceive{{before:{label},occurrence,actor:{actor},role:{role},label}})=probe && occurrence==script.first_occurrence(){{let result=core::future::poll_fn(|cx|core::task::Poll::Ready(match label{{{arms}_=>panic!(\"unknown causal probe\")}})).await;match result{{core::task::Poll::Ready(Err(error))=>{{observed.scope_rejections.borrow_mut().push(probe);return Err(error)}},other=>panic!(\"send before required receive was not rejected: script={{script_index}} reverse={{reverse_poll}} {{probe:?}} {{other:?}} prefix={{:?}}\",observed.completed.borrow())}}}}"
                );
            }
        }
        let code =
            format!("tokio::task::yield_now().await;let observed_role={observe};{causal}{call}");
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
        primed: Option<Entry>,
    ) -> String {
        let primed = primed.filter(|entry| contains(source, entry.label()));
        match source {
            Protocol::Message {
                from, to, label, ..
            } => {
                let mut code = String::new();
                if from == to {
                    code += &match primed {
                        Some(Entry::Consumed(_)) => String::new(),
                        Some(Entry::LocalPreview(_)) => self.operation(source, actor, names, true),
                        None => self.operation(source, actor, names, false),
                    };
                } else {
                    assert!(!matches!(primed, Some(Entry::LocalPreview(_))));
                    if primed != Some(Entry::Consumed(*label)) {
                        code += &self.operation(source, actor, names, false);
                    }
                    code += &self.operation(source, actor, names, true);
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
                if matches!(body.as_ref(), Protocol::Par(..)) {
                    let (declarations, checks, repeat, returns) =
                        self.parallel_code(body, actor, names, None);
                    if repeat.is_empty() {
                        return repeat;
                    }
                    let assign = if returns { "db=" } else { "" };
                    let visits = if primed.is_some() {
                        let (_, first_checks, first, first_returns) =
                            self.parallel_code(body, actor, names, primed);
                        assert_eq!(returns, first_returns);
                        format!(
                            "{{let [_first,_second]=match script{{Script::Roll(visits)=>[visits[0].as_ref(),visits[1].as_ref()],_=>panic!(\"expected repeated script\")}};{{let script=_first;{first_checks}{assign}{first};}}{{let script=_second;{checks}{assign}{repeat};}}}}"
                        )
                    } else {
                        format!(
                            "for visit in match script{{Script::Roll(visits)=>visits,_=>panic!(\"expected repeated script\")}}{{let script=visit.as_ref();{checks}{assign}{repeat};}}"
                        )
                    };
                    return format!("{{{declarations}{visits}}}");
                }
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
                        .find_map(|(node, owners)| core::ptr::eq(*node, source).then_some(*owners))
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
        primed: Option<Entry>,
    ) -> String {
        let (a, b) = match source {
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => (a, b),
            _ => unreachable!(),
        };
        let select = "let (script,right)=match script{Script::Branch(body,right)=>(body.as_ref(),*right),_=>panic!(\"expected branch script\")};";
        if let Some(entry) = primed {
            let label = entry.label();
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
                let allowed = entries
                    .iter()
                    .map(|message| match message {
                        Protocol::Message { label, .. } => label.to_string(),
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let mut previewed = std::collections::BTreeSet::new();
                for message in &entries {
                    let Protocol::Message { label, .. } = message else {
                        unreachable!()
                    };
                    let decisions = self.paths[label]
                        .iter()
                        .map(|(id, right)| format!("decisions[{id}].set(Some({right}));"))
                        .collect::<String>();
                    if matches!(source, Protocol::Route(..)) || message_selected_head(body, *label)
                    {
                        let send = self.operation(message, actor, names, false);
                        let rest = self.emit(body, actor, names, Some(Entry::Consumed(*label)));
                        arms += &format!("{label}=>{{{decisions}{send}{rest}}},");
                    } else if previewed.insert(self.paths[label].clone()) {
                        let alternatives = entries
                            .iter()
                            .filter_map(|message| match message {
                                Protocol::Message { label: other, .. }
                                    if !message_selected_head(body, *other)
                                        && self.paths[other] == self.paths[label] =>
                                {
                                    Some(other.to_string())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("|");
                        let mut preview =
                            preview_for(controller_role, *label, &format!("&[{allowed}][..]"));
                        let mut rest = self.emit(body, actor, names, None);
                        if entries
                            .iter()
                            .any(|m| matches!(m,Protocol::Message{from,to,..} if from==to))
                        {
                            // Preserve the local preview until its own fragment
                            // receives it, including across a parallel borrow.
                            let endpoint = self.endpoint(controller_role, names);
                            preview = format!(
                                "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{})).await?;let label=branch.label();drop(branch);let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{})).await?;assert_eq!(branch.label(),label);",
                                label, label
                            );
                            let mut local_arms = String::new();
                            for entry in &entries {
                                let Protocol::Message {
                                    from,
                                    to,
                                    label: entry_label,
                                    ..
                                } = entry
                                else {
                                    unreachable!()
                                };
                                if from == to {
                                    let remaining = self.emit(
                                        body,
                                        actor,
                                        names,
                                        Some(Entry::LocalPreview(*entry_label)),
                                    );
                                    local_arms +=
                                        &format!("{entry_label}=>{{drop(branch);{remaining}}},");
                                } else {
                                    let remaining = self.emit(body, actor, names, None);
                                    local_arms +=
                                        &format!("{entry_label}=>{{drop(branch);{remaining}}},");
                                }
                            }
                            rest = format!(
                                "match label{{{local_arms}_=>panic!(\"unknown local branch preview\")}}"
                            );
                        }
                        arms += &format!("{alternatives}=>{{{decisions}{preview}{rest}}},");
                    }
                }
                format!("match script.trace()[0].0{{{arms}_=>panic!(\"unknown branch entry\")}}")
            } else {
                self.emit(body, actor, names, None)
            }
        };
        let left = arm(a);
        let right = arm(b);
        if actor == controller {
            return format!("{{{select}if right{{{right}}}else{{{left}}}}}");
        }
        let left_entries = first_local(a, actor, before);
        let right_entries = first_local(b, actor, before);
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
                .map(|(_, label)| label.to_string())
                .collect::<Vec<_>>()
                .join(",");
            preview_for(view, entry, &format!("&[{allowed}][..]"))
        } else {
            let mut offers = String::new();
            let mut retake = String::new();
            for (view, entry) in views {
                let endpoint = self.endpoint(view, names);
                offers += &format!(
                    "candidate=async{{let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;let label=branch.label();drop(branch);Ok::<_,hibana::EndpointError>(label)}}=>candidate?,"
                );
                let labels = left_entries
                    .iter()
                    .chain(&right_entries)
                    .filter(|(role, _)| *role == view)
                    .map(|(_, label)| *label)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .map(|label| label.to_string())
                    .collect::<Vec<_>>()
                    .join("|");
                retake += &format!(
                    "{labels}=>{{let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;assert_eq!(branch.label(),label);drop(branch);}},"
                );
            }
            format!(
                "let label=tokio::select!{{{offers}}};match label{{{retake}_=>panic!(\"unknown incoming view\")}}"
            )
        };
        let mut branches = String::new();
        for (entries, selected, body) in [(left_entries, false, left), (right_entries, true, right)]
        {
            let labels = entries
                .into_iter()
                .map(|(_, label)| label)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|label| label.to_string())
                .collect::<Vec<_>>()
                .join("|");
            if !labels.is_empty() {
                branches += &format!("{labels}=>{{assert_eq!(right,{selected});{body}}},");
            }
        }
        format!(
            "{{{select}{preview}match label{{{branches}_=>panic!(\"unknown received branch\")}}}}"
        )
    }

    fn parallel_code(
        &self,
        source: &Protocol,
        actor: u8,
        names: &[String; 4],
        primed: Option<Entry>,
    ) -> (String, String, String, bool) {
        let mut parts = Vec::new();
        parallel_parts(source, &mut parts);
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
            let mut inner = Vec::new();
            let bindings = indices
                .iter()
                .map(|i| {
                    let used = present(parts[*i], role as u8, primed.and_then(Entry::consumed));
                    let name = format!("{}p{role}_{i}", if used { "" } else { "_" });
                    part_names[*i][role] = name.clone();
                    inner.push(name.clone());
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
            names: &[[String; 4]],
            part: &mut usize,
            primed: Option<Entry>,
        ) -> (String, bool) {
            if let Protocol::Par(a, b) = source {
                let (left, returns_left) = schedule(e, a, actor, names, part, primed);
                let (right, returns_right) = schedule(e, b, actor, names, part, primed);
                assert!(!returns_left || !returns_right);
                if left.is_empty() && right.is_empty() {
                    return (String::new(), false);
                }
                let bind = |plan: &str, body: &str| {
                    if body.is_empty() {
                        "async{Ok::<_,hibana::EndpointError>(())}".to_owned()
                    } else {
                        format!(
                            "async{{let script={plan};let result={body};Ok::<_,hibana::EndpointError>(result)}}"
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
                        "{{let (_left,_right,reverse)=match script{{Script::Par(a,b,reverse)=>(a.as_ref(),b.as_ref(),*reverse),_=>panic!(\"expected parallel script\")}};let left={};let right={};let (_a,_b)=if reverse ^ reverse_poll{{let (b,a)=tokio::try_join!(right,left)?;(a,b)}}else{{tokio::try_join!(left,right)?}};{result}}}",
                        bind("_left", &left),
                        bind("_right", &right)
                    ),
                    returns_left || returns_right,
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
        let (body, returns) = schedule(self, source, actor, &part_names, &mut 0, primed);
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
    let mut code = format!(
        r#"
    let mut completed_plan_words=std::collections::BTreeSet::new();
    let script_range=selected_script_range({script_count});
    let scripts=std::io::BufRead::lines(std::io::BufReader::new(std::fs::File::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/{case}.jsonl")).expect("missing script plans")));
    use std::io::Write as _;
    let scope_root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut scope_plans=ScopePlans::new(std::io::BufReader::new(std::fs::File::open(scope_root.join("scope-plans/{case}.jsonl")).expect("missing scope plans")),{case},{});
    std::fs::create_dir_all(scope_root.join("scope-observations")).unwrap();
    let mut scope_observations=std::io::BufWriter::new(std::fs::File::create(scope_root.join("scope-observations/{case}.jsonl")).unwrap());
    writeln!(scope_observations,"{{{{\"name\":{case},\"scripts\":{}}}}}").unwrap();
    let mut checked_scripts=0usize;
    for (script_index,line) in scripts.enumerate() {{ let decoded_script=Script::from_json(&line.expect("unreadable script plan")).expect("invalid script plan");let script=&decoded_script;let planned_probes=scope_plans.next(script_index);let record_script=script_range.contains(&script_index);let probes=if record_script{{planned_probes.as_slice()}}else{{&[]}};for reverse_poll in [false,true] {{
    if record_script{{write!(scope_observations,"{{{{\"script\":{{script_index}},\"reversePoll\":{{reverse_poll}}").unwrap();}}
    let mut scoped_rejections=Vec::new();
    for probe in core::iter::once(None).chain(probes.iter().copied().map(Some)) {{
      let _=reverse_poll;
      let decisions:Vec<std::cell::Cell<Option<bool>>>=(0..{}).map(|_|std::cell::Cell::new(None)).collect();let _=&decisions;
      let sid=SessionId::new(2);
      // SAFETY: this execution owns its resource and its only initial issuer.
      let network=InMemoryTransport::new(programs.resource.then(||((sid,3),unsafe{{EndpointBinding::new()}})));
      let [mut slab0,mut slab1,mut slab2,mut slab3]=[vec![0;128*1024],vec![0;128*1024],vec![0;128*1024],vec![0;128*1024]];
      let mut s0=SessionKitStorage::uninit();let mut s1=SessionKitStorage::uninit();let mut s2=SessionKitStorage::uninit();let mut s3=SessionKitStorage::uninit();
      let rv0=s0.init().rendezvous(&mut slab0,network.clone()).unwrap();
      let rv1=s1.init().rendezvous(&mut slab1,network.clone()).unwrap();
      let rv2=s2.init().rendezvous(&mut slab2,network.clone()).unwrap();
      let rv3=s3.init().rendezvous(&mut slab3,network.clone()).unwrap();
    "#,
        script_count,
        script_count,
        resolvers.len()
    );
    for id in resolvers {
        code += &format!("match programs.resolver_roles[{id}]{{");
        for actor in 0..3 {
            code += &format!(
                "{actor}=>rv{actor}.set_resolver(programs.p{actor}.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&decisions,|d|Ok(if d[{id}].get().expect(\"unplanned choice\"){{DecisionArm::Right}}else{{DecisionArm::Left}}))).unwrap(),"
            );
        }
        code += "3=>{";
        for actor in 0..4 {
            code += &format!(
                "if {}{{rv{actor}.set_resolver(programs.p3.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&decisions,|d|Ok(if d[{id}].get().expect(\"unplanned choice\"){{DecisionArm::Right}}else{{DecisionArm::Left}}))).unwrap();}}",
                if actor == 3 {
                    "!programs.resource"
                } else {
                    "programs.resource"
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
        let delay = if actor == owner {
            "tokio::time::sleep(core::time::Duration::from_millis(1)).await;"
        } else {
            ""
        };
        code += &format!(
            "let run{actor}=async{{{db}{delay}{body}{}Ok::<_,hibana::EndpointError>(())}};",
            if mobile(source) && actor < 3 {
                "drop(db);"
            } else {
                ""
            }
        );
    }
    code += r#"let result=tokio::time::timeout(core::time::Duration::from_secs(1),async{tokio::try_join!(run0,run1,run2,run3)}).await.expect("generated localside stalled");
    if let Some(probe)=probe{assert!(result.is_err(),"local probe did not stop its run");let actual=observed.scope_rejections.borrow();assert_eq!(*actual,[probe],"planned local probe was not reached");scoped_rejections.push(actual[0].json());continue;}
    result.unwrap();
    if record_script{write!(scope_observations,",\"completed\":[").unwrap();
    for (index,(actor,role,label,receive)) in observed.completed.borrow().iter().enumerate(){if index!=0{write!(scope_observations,",").unwrap();}write!(scope_observations,"[{actor},{role},{label},{receive}]").unwrap();}
    write!(scope_observations,"]").unwrap();}
    assert!(observed.calls.borrow().is_empty());
    "#;
    code += &format!(
        r#"}}if record_script{{writeln!(scope_observations,",\"probes\":[{{}}]}}}}",scoped_rejections.join(",")).unwrap();}}}}completed_plan_words.insert(script.line());checked_scripts+=1;}}assert_eq!(checked_scripts,{},"missing script plan");scope_plans.finish();scope_observations.flush().unwrap();"#,
        script_count
    );
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from: u8, to: u8, owned: bool) -> Protocol {
        Protocol::Message {
            from,
            to,
            label: 0,
            owned,
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
}
