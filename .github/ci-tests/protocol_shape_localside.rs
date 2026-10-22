//! Emit typed localsides from the same immutable syntax as the test input.
use super::{Execution, Protocol};

fn contains(source: &Protocol, label: u8) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { label: candidate, .. } if *candidate == label))
}

fn mobile(source: &Protocol) -> bool {
    let mut messages = Vec::new();
    source.messages(&mut messages);
    messages
        .iter()
        .any(|m| matches!(m, Protocol::Message { owned: true, .. }))
}

fn final_holder(source: &Protocol, holder: u8) -> u8 {
    match source {
        Protocol::Message {
            from,
            to,
            owned: true,
            ..
        } => {
            assert_eq!(holder, *from);
            *to
        }
        Protocol::Message { .. } => holder,
        Protocol::Seq(a, b) => final_holder(b, final_holder(a, holder)),
        Protocol::Par(a, b) => {
            assert!(!mobile(a) || !mobile(b));
            if mobile(a) {
                final_holder(a, holder)
            } else {
                final_holder(b, holder)
            }
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let after = final_holder(a, holder);
            assert_eq!(after, final_holder(b, holder));
            after
        }
        Protocol::Roll(body) => {
            assert_eq!(holder, final_holder(body, holder));
            holder
        }
    }
}

fn present(source: &Protocol, role: u8, primed: Option<u8>) -> bool {
    match source {
        Protocol::Message {
            from, to, label, ..
        } => primed != Some(*label) && [*from, *to].contains(&role),
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

// A moving parallel child declares its received value inside its own future.
// Only receives outside such children need a binding in the enclosing scope.
fn receives_here(source: &Protocol, actor: u8) -> bool {
    match source {
        Protocol::Message { to, owned, .. } => *owned && *to == actor,
        Protocol::Par(..) if mobile(source) => false,
        Protocol::Seq(a, b)
        | Protocol::Par(a, b)
        | Protocol::Route(a, b)
        | Protocol::Resolved(a, b, _) => receives_here(a, actor) || receives_here(b, actor),
        Protocol::Roll(body) => receives_here(body, actor),
    }
}

fn db_activity(source: &Protocol, actor: u8, holder: u8) -> (usize, bool) {
    match source {
        Protocol::Message {
            from, to, owned, ..
        } => (
            usize::from(*owned && *to == actor),
            !owned && holder == actor && [*from, *to].contains(&3),
        ),
        Protocol::Seq(a, b) => {
            let a_activity = db_activity(a, actor, holder);
            let b_activity = db_activity(b, actor, final_holder(a, holder));
            (a_activity.0 + b_activity.0, a_activity.1 || b_activity.1)
        }
        Protocol::Par(..) if mobile(source) => (usize::from(actor == holder), false),
        Protocol::Par(a, b) => {
            let a = db_activity(a, actor, holder);
            let b = db_activity(b, actor, holder);
            (a.0 + b.0, a.1 || b.1)
        }
        Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
            let a = db_activity(a, actor, holder);
            let b = db_activity(b, actor, holder);
            (a.0.max(b.0), a.1 || b.1)
        }
        Protocol::Roll(body) => {
            let (assignments, borrow) = db_activity(body, actor, holder);
            (2 * assignments, borrow)
        }
    }
}

fn parallel_parts<'a>(source: &'a Protocol, parts: &mut Vec<&'a Protocol>) {
    if let Protocol::Par(a, b) = source {
        parallel_parts(a, parts);
        parallel_parts(b, parts);
    } else {
        parts.push(source);
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
                first_local(b, actor, final_holder(a, holder))
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

struct Emitter {
    paths: std::collections::BTreeMap<u8, Vec<(u16, bool)>>,
}

impl Emitter {
    fn operation(
        &self,
        message: &Protocol,
        actor: u8,
        holder: u8,
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
        if (if role == 3 { holder } else { role }) != actor {
            return String::new();
        }
        let endpoint = &names[usize::from(role)];
        let call = if receive {
            format!("{endpoint}.recv::<M{label}<'_>>()")
        } else {
            format!(
                "{endpoint}.send::<M{label}<'_>>({})",
                if *owned { "db" } else { "&()" }
            )
        };
        format!(
            "tokio::task::yield_now().await;{}record_pending({call},observed,ObservedCall::Message({actor},{label},{receive})).await?;",
            if receive && *owned { "db=" } else { "" }
        )
    }

    fn emit(
        &self,
        source: &Protocol,
        actor: u8,
        holder: &mut u8,
        names: &[String; 4],
        primed: Option<u8>,
    ) -> String {
        let primed = primed.filter(|label| contains(source, *label));
        match source {
            Protocol::Message {
                from,
                to,
                label,
                owned,
            } => {
                let mut code = String::new();
                if primed != Some(*label) {
                    code += &self.operation(source, actor, *holder, names, false);
                }
                code += &self.operation(source, actor, *holder, names, true);
                if *owned {
                    assert_eq!(*holder, *from);
                    *holder = *to;
                }
                if code.is_empty() {
                    code
                } else {
                    format!("assert!(matches!(script,Script::Message({label})));{code}")
                }
            }
            Protocol::Seq(a, b) => {
                let a = self.emit(a, actor, holder, names, primed);
                let b = self.emit(b, actor, holder, names, primed);
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
                    "{{let (_left,_right)=match script{{Script::Seq(a,b)=>(*a,*b),_=>panic!(\"expected sequential script\")}};{}{}}}",
                    bind("_left", &a),
                    bind("_right", &b)
                )
            }
            Protocol::Roll(body) => {
                let before = *holder;
                let first = self.emit(body, actor, holder, names, primed);
                assert_eq!(*holder, before);
                if primed.is_some() {
                    let second = self.emit(body, actor, holder, names, None);
                    if first.is_empty() && second.is_empty() {
                        return String::new();
                    }
                    let first = if first.is_empty() {
                        String::new()
                    } else {
                        format!("{{let script=_first;{first}}}")
                    };
                    format!(
                        "{{let [_first,_second]=match script{{Script::Roll(visits)=>*visits,_=>panic!(\"expected repeated script\")}};{first}{{let script=_second;{second}}}}}"
                    )
                } else if first.is_empty() {
                    String::new()
                } else {
                    format!(
                        "for &script in match script{{Script::Roll(visits)=>visits,_=>panic!(\"expected repeated script\")}}{{{first}}}"
                    )
                }
            }
            Protocol::Route(a, b) | Protocol::Resolved(a, b, _) => {
                let before = *holder;
                *holder = final_holder(source, before);
                let select = "let (script,right)=match script{Script::Branch(body,right)=>(*body,*right),_=>panic!(\"expected branch script\")};";
                if let Some(label) = primed {
                    let right = contains(b, label);
                    let mut current = before;
                    let body = self.emit(
                        if right { b } else { a },
                        actor,
                        &mut current,
                        names,
                        primed,
                    );
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
                let controller = match entry[0] {
                    Protocol::Message { from, .. } => {
                        if *from == 3 {
                            before
                        } else {
                            *from
                        }
                    }
                    _ => unreachable!(),
                };
                let arm = |body: &Protocol| {
                    if actor == controller {
                        let mut entries = Vec::new();
                        heads(body, &mut entries);
                        let mut arms = String::new();
                        for message in entries {
                            let Protocol::Message { label, from, .. } = message else {
                                unreachable!()
                            };
                            assert_eq!(if *from == 3 { before } else { *from }, controller);
                            let decisions = self.paths[label]
                                .iter()
                                .map(|(id, right)| format!("decisions[{id}].set(Some({right}));"))
                                .collect::<String>();
                            let send = self.operation(message, actor, before, names, false);
                            let mut current = before;
                            let rest = self.emit(body, actor, &mut current, names, Some(*label));
                            arms += &format!("{label}=>{{{decisions}{send}{rest}}},");
                        }
                        format!(
                            "match script.trace()[0].0{{{arms}_=>panic!(\"unknown branch entry\")}}"
                        )
                    } else {
                        let mut current = before;
                        self.emit(body, actor, &mut current, names, None)
                    }
                };
                let left = arm(a);
                let right = arm(b);
                let preview = if actor == controller {
                    String::new()
                } else {
                    let view = first[0].0;
                    assert!(first.iter().all(|(other, _)| *other == view));
                    let labels = |body| {
                        first_local(body, actor, before)
                            .into_iter()
                            .map(|(_, label)| label.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    let endpoint = &names[usize::from(view)];
                    let entry = first[0].1;
                    format!(
                        "let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;let label=branch.label();drop(branch);let branch=record_pending({endpoint}.offer(),observed,ObservedCall::Preview({actor},{entry})).await?;assert_eq!(branch.label(),label);drop(branch);assert!((if right{{&[{}][..]}}else{{&[{}][..]}}).contains(&label));",
                        labels(b),
                        labels(a)
                    )
                };
                format!("{{{select}{preview}if right{{{right}}}else{{{left}}}}}")
            }
            Protocol::Par(..) => {
                let before = *holder;
                *holder = final_holder(source, before);
                let mut parts = Vec::new();
                parallel_parts(source, &mut parts);
                let mut part_names = vec![names.clone(); parts.len()];
                let mut declarations = String::new();
                for role in 0..4 {
                    if (if role == 3 { usize::from(before) } else { role }) != usize::from(actor)
                        || (role == 3 && mobile(source))
                    {
                        continue;
                    }
                    let indices = (0..parts.len())
                        // Borrow width belongs to the projected source group,
                        // including an entry already sent to choose its route.
                        .filter(|i| present(parts[*i], role as u8, None))
                        .collect::<Vec<_>>();
                    if indices.is_empty() {
                        continue;
                    }
                    let bindings = indices
                        .iter()
                        .map(|i| {
                            let used = present(parts[*i], role as u8, primed);
                            let name = format!("{}p{role}_{i}", if used { "" } else { "_" });
                            part_names[*i][role] = name.clone();
                            format!("{}{name}", if used { "mut " } else { "" })
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    declarations += &format!("let [{bindings}]={}.par()?;", names[role]);
                }
                fn schedule(
                    e: &Emitter,
                    source: &Protocol,
                    actor: u8,
                    holder: u8,
                    names: &[[String; 4]],
                    part: &mut usize,
                    primed: Option<u8>,
                ) -> (String, bool) {
                    if let Protocol::Par(a, b) = source {
                        let (left, returns_left) =
                            schedule(e, a, actor, holder, names, part, primed);
                        let (right, returns_right) =
                            schedule(e, b, actor, holder, names, part, primed);
                        assert!(!returns_left || !returns_right);
                        if left.is_empty() && right.is_empty() {
                            return (String::new(), false);
                        }
                        let bind = |plan: &str, body: &str| {
                            if body.is_empty() {
                                "async{Ok::<_,hibana::EndpointError>(())}".into()
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
                                "{{let (_left,_right,reverse)=match script{{Script::Par(a,b,reverse)=>(*a,*b,*reverse),_=>panic!(\"expected parallel script\")}};let left={};let right={};let (_a,_b)=if concurrent{{tokio::try_join!(left,right)?}}else if reverse{{let b=right.await?;let a=left.await?;(a,b)}}else{{let a=left.await?;let b=right.await?;(a,b)}};{result}}}",
                                bind("_left", &left),
                                bind("_right", &right)
                            ),
                            returns_left || returns_right,
                        )
                    } else {
                        let names = &names[*part];
                        *part += 1;
                        let mut current = holder;
                        let body = e.emit(source, actor, &mut current, names, primed);
                        assert_eq!(current, holder);
                        if body.is_empty() {
                            return (body, false);
                        }
                        let owned = mobile(source);
                        let returns = owned && actor == holder;
                        let already_moved=primed.is_some_and(|label| {
                            let mut messages=Vec::new();source.messages(&mut messages);
                            messages.iter().any(|m|matches!(m,Protocol::Message {label:candidate,owned:true,from,..} if *candidate==label && *from==actor))
                        });
                        let activity = db_activity(source, actor, holder);
                        let uninitialized = if activity.0 > 1 || activity.1 {
                            "let mut db;"
                        } else {
                            "let db;"
                        };
                        let init =
                            if owned && actor < 2 && (returns || receives_here(source, actor)) {
                                if returns && !already_moved {
                                    "let mut db=db;"
                                } else {
                                    uninitialized
                                }
                            } else {
                                ""
                            };
                        (
                            format!("{{{init}{body}{}}}", if returns { "db" } else { "()" }),
                            returns,
                        )
                    }
                }
                let (body, returns) =
                    schedule(self, source, actor, before, &part_names, &mut 0, primed);
                if body.is_empty() {
                    body
                } else {
                    format!(
                        "{{{declarations}{}{body};}}",
                        if returns { "db=" } else { "" }
                    )
                }
            }
        }
    }
}

pub(super) fn runs(source: &Protocol, owner: u8, third_actor: bool) -> String {
    let executions = source.executions();
    let mut paths = std::collections::BTreeMap::new();
    source.resolver_choices(&mut Vec::new(), &mut paths);
    let e = Emitter { paths };
    let resolvers = e
        .paths
        .values()
        .flatten()
        .map(|(id, _)| *id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut code = format!(
        r#"
    let mut completed_scoped_traces=std::collections::BTreeSet::new();
    let scripts:&[Script]=&[{}];
    for script in scripts {{ for concurrent in [false,true] {{
      let _=concurrent;
      let decisions:Vec<std::cell::Cell<Option<bool>>>=(0..{}).map(|_|std::cell::Cell::new(None)).collect();let _=&decisions;
      let sid=SessionId::new(2);
      // SAFETY: this execution owns its resource and its only initial issuer.
      let network=InMemoryTransport::new(programs.p3.as_ref().map(|_|((sid,3),unsafe{{EndpointBinding::new()}})));
      let [mut slab0,mut slab1,mut slab2]=[vec![0;128*1024],vec![0;128*1024],vec![0;128*1024]];
      let mut s0=SessionKitStorage::uninit();let mut s1=SessionKitStorage::uninit();let mut s2=SessionKitStorage::uninit();
      let rv0=s0.init().rendezvous(&mut slab0,network.clone()).unwrap();
      let rv1=s1.init().rendezvous(&mut slab1,network.clone()).unwrap();
      let rv2=s2.init().rendezvous(&mut slab2,network.clone()).unwrap();
    "#,
        executions
            .iter()
            .map(Execution::rust)
            .collect::<Vec<_>>()
            .join(","),
        resolvers.len()
    );
    for id in resolvers {
        code += &format!(
            "match programs.resolver_roles[{id}]{{0=>rv0.set_resolver(&programs.p0,ResolverRef::<{id}>::decision_state(&decisions,|d|Ok(if d[{id}].get().expect(\"unplanned choice\"){{DecisionArm::Right}}else{{DecisionArm::Left}}))).unwrap(),1=>rv1.set_resolver(&programs.p1,ResolverRef::<{id}>::decision_state(&decisions,|d|Ok(if d[{id}].get().expect(\"unplanned choice\"){{DecisionArm::Right}}else{{DecisionArm::Left}}))).unwrap(),3=>{{"
        );
        for actor in 0..2 {
            code += &format!(
                "rv{actor}.set_resolver(programs.p3.as_ref().unwrap(),ResolverRef::<{id}>::decision_state(&decisions,|d|Ok(if d[{id}].get().expect(\"unplanned choice\"){{DecisionArm::Right}}else{{DecisionArm::Left}}))).unwrap();"
            );
        }
        code += "},_=>panic!(\"unknown choosing role\")};";
    }
    code += "let mut e0=rv0.enter(sid,&programs.p0).unwrap();let mut e1=rv1.enter(sid,&programs.p1).unwrap();";
    if third_actor {
        code += "let mut e2=rv2.enter(sid,&programs.p2).unwrap();";
    } else {
        code += "let _=&rv2;";
    }
    if mobile(source) {
        code +=
            &format!("let initial_db=rv{owner}.enter(sid,programs.p3.as_ref().unwrap()).unwrap();");
    }
    code += "let observed=&PendingCalls::default();";
    for actor in 0..3 {
        let db = if !mobile(source) || actor == 2 {
            ""
        } else if actor == owner {
            "let mut db=initial_db;"
        } else if receives_here(source, actor) {
            let activity = db_activity(source, actor, owner);
            if activity.0 > 1 || activity.1 {
                "let mut db;"
            } else {
                "let db;"
            }
        } else {
            ""
        };
        let mut holder = owner;
        let body = e.emit(
            source,
            actor,
            &mut holder,
            &["e0".into(), "e1".into(), "e2".into(), "db".into()],
            None,
        );
        let delay = if actor == owner {
            "tokio::time::sleep(core::time::Duration::from_millis(1)).await;"
        } else {
            ""
        };
        code += &format!(
            "let run{actor}=async{{{db}{delay}{body}{}Ok::<_,hibana::EndpointError>(())}};",
            if mobile(source) && actor == owner {
                "drop(db);"
            } else {
                ""
            }
        );
    }
    code += r#"tokio::time::timeout(core::time::Duration::from_secs(1),async{tokio::try_join!(run0,run1,run2)}).await.expect("generated localside stalled").unwrap();
    assert!(observed.calls.borrow().is_empty());
    let mut actual=observed.completed.borrow().clone();actual.sort();
    let mut expected=Vec::new();let mut holder=programs.owner;
    for (label,_) in script.trace(){let message=messages_for_scoped_runs.iter().find(|m|m.label==label).unwrap();
      expected.push((if message.from==3{holder}else{message.from},label,false));
      expected.push((if message.to==3{holder}else{message.to},label,true));
      if message.owned{assert_eq!(holder,message.from);holder=message.to;}
    }
    expected.sort();assert_eq!(actual,expected,"scoped execution omitted or repeated an operation");
    "#;
    if matches!(source, Protocol::Par(..)) {
        code += "if concurrent{assert!(observed.overlapped.get(),\"independent borrows never overlapped while pending\");}";
    }
    code += "}completed_scoped_traces.insert(script.line());}";
    code
}
