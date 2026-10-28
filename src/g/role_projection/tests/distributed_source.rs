//! Passive measurements of one execution, without changing its localside.
use crate::{endpoint::proof_snapshot::ActorSnapshot, test_support::transport::FrameEvidence};

#[cfg(not(miri))]
pub(super) const DISTRIBUTED_DECIDE: &str = concat!(
    "((try simp only [sourceReceivePairCheck, sourcePublicationPairCheck, ",
    "sourceOperationIdentity?, SourceSendAdmission.check, ",
    "SourceSendAdmission.payloadMatches, SourceSendAdmission.ownedFacts?, ",
    "ActorCommitRecord.check, ActorCommitObservation.ownedCheck, ",
    "ActorCommitObservation.check, ActorCommitObservation.ownedFacts?, ",
    "RustDescriptorImage.ownedSourceFacts?, ",
    "ActorReentryCommitObservation.prepared?, prepareObservedActorBody?, ",
    "distributedWordData0Decoded, distributedWordData1Decoded, ",
    "distributedWordData3Decoded, distributedWordRolls0Decoded, ",
    "distributedWordRolls1Decoded, distributedWordRolls3Decoded, ",
    "distributedCanonicalRolls0Decoded, distributedCanonicalRolls1Decoded, ",
    "distributedCanonicalRolls3Decoded, distributedActorOrder0Checked, ",
    "distributedActorOrder1Checked, distributedActorOrder3Checked, ",
    "distributedActorRoll0Checked, distributedActorRoll1Checked, ",
    "distributedActorRoll3Checked, distributedActorRoute0Checked, ",
    "distributedActorRoute1Checked, distributedActorRoute3Checked]) <;> (first | rfl | decide +kernel))",
);

pub(super) enum FrameBoundary {
    Sent(FrameEvidence),
    Received(FrameEvidence),
}

pub(super) struct OperationObservation {
    pub(super) view: u8,
    pub(super) label: u8,
    pub(super) before: ActorSnapshot,
    pub(super) after: ActorSnapshot,
    pub(super) frame: FrameBoundary,
}

impl OperationObservation {
    pub(super) fn frame(&self) -> &FrameEvidence {
        match &self.frame {
            FrameBoundary::Sent(frame) | FrameBoundary::Received(frame) => frame,
        }
    }
}

pub(super) fn check_observations(operations: &[OperationObservation]) {
    use std::collections::BTreeMap;
    let mut accepted = BTreeMap::new();
    let mut delivered = std::collections::BTreeSet::new();
    let mut actors = BTreeMap::new();
    for operation in operations {
        let before = &operation.before;
        let after = &operation.after;
        assert_eq!(before.actor, after.actor);
        if let Some(previous) = actors.insert(before.actor, after) {
            assert_eq!(previous, before, "missing actor boundary");
        }
        let frame = operation.frame();
        let event = before
            .steps
            .iter()
            .enumerate()
            .position(|(index, step)| {
                operation.after.steps[index].done
                    && step.view == operation.view
                    && match (&operation.frame, step.action) {
                        (FrameBoundary::Sent(_), super::LocalAction::Send { label, .. })
                        | (FrameBoundary::Received(_), super::LocalAction::Recv { label, .. }) => {
                            label == operation.label
                        }
                        _ => false,
                    }
            })
            .expect("source operation");
        assert!(after.steps[event].done);
        assert!(after.completion_mutations > before.completion_mutations);
        match &operation.frame {
            FrameBoundary::Sent(_) => {
                assert_eq!(frame.header.source_role(), operation.view);
                assert!(accepted.insert(frame.witness, operation).is_none());
            }
            FrameBoundary::Received(_) => {
                assert_eq!(frame.header.target_role(), operation.view);
                assert!(
                    delivered.insert(frame.witness),
                    "duplicate physical receive"
                );
                let source = accepted.get(&frame.witness).expect("actual acceptance");
                assert_eq!(source.frame().header, frame.header);
                assert_eq!(source.frame().payload(), frame.payload());
                assert_eq!(source.label, operation.label);
            }
        }
    }

    assert_eq!(delivered.len(), accepted.len());
    assert_eq!(operations.len(), accepted.len() * 2);
}

#[cfg(not(miri))]
fn source_message(source: &crate::g::ProgramSourceNode, event: usize) -> std::string::String {
    let message = source.message_at(event);
    let key = |key: crate::g::EndpointKey| {
        std::format!("{{ component := {}, role := {} }}", key.component, key.role)
    };
    let payload = match message.payload {
        crate::g::SourcePayload::Wire { schema } => std::format!(".wire {schema}"),
        crate::g::SourcePayload::Endpoint(endpoint) => {
            std::format!(".endpoint {}", key(endpoint))
        }
    };
    std::format!(
        "{{ sender := {}, receiver := {}, label := {}, payload := {payload} }}",
        key(message.from),
        key(message.to),
        message.label,
    )
}

#[cfg(not(miri))]
fn ownership_source(source: &crate::g::ProgramSourceNode) -> std::string::String {
    use crate::g::ProgramSourceNode as Source;
    match source {
        Source::Send(_) => std::format!(".send {}", source_message(source, 0)),
        Source::Seq(left, right, _) | Source::Par(left, right, _) => std::format!(
            ".{} ({}) ({})",
            if matches!(source, Source::Seq(..)) {
                "seq"
            } else {
                "par"
            },
            ownership_source(left),
            ownership_source(right),
        ),
        Source::Route {
            left,
            right,
            resolver,
            ..
        } => std::format!(
            ".route ({}) ({}) ({})",
            resolver.map_or_else(|| "none".into(), |id| std::format!("some {id}")),
            ownership_source(left),
            ownership_source(right),
        ),
        Source::Roll(body, _) => std::format!(".roll ({})", ownership_source(body)),
    }
}

#[cfg(not(miri))]
fn source_routes(
    source: &crate::g::ProgramSourceNode,
    start: usize,
    next_scope: &mut u16,
    routes: &mut std::vec::Vec<(usize, usize, usize)>,
    ordinals: &mut std::vec::Vec<u16>,
) {
    use crate::g::ProgramSourceNode as Source;
    match source {
        Source::Send(_) => {}
        Source::Seq(left, right, _) | Source::Par(left, right, _) => {
            if matches!(source, Source::Par(..)) {
                *next_scope += 1;
            }
            source_routes(left, start, next_scope, routes, ordinals);
            source_routes(
                right,
                start + left.event_count(),
                next_scope,
                routes,
                ordinals,
            );
        }
        Source::Route { left, right, .. } => {
            let split = start + left.event_count();
            ordinals.push(*next_scope);
            *next_scope += 1;
            routes.push((start, split, split + right.event_count()));
            source_routes(left, start, next_scope, routes, ordinals);
            source_routes(right, split, next_scope, routes, ordinals);
        }
        Source::Roll(body, _) => {
            *next_scope += 1;
            source_routes(body, start, next_scope, routes, ordinals);
        }
    }
}

#[cfg(not(miri))]
fn selected_events(
    source: &crate::g::ProgramSourceNode,
    start: usize,
    routes: &[(usize, usize, usize)],
    selected: &[u8],
    events: &mut std::vec::Vec<usize>,
) {
    use crate::g::ProgramSourceNode as Source;
    match source {
        Source::Send(_) => events.push(start),
        Source::Seq(left, right, _) | Source::Par(left, right, _) => {
            selected_events(left, start, routes, selected, events);
            selected_events(right, start + left.event_count(), routes, selected, events);
        }
        Source::Route { left, right, .. } => {
            let split = start + left.event_count();
            let scope = (start, split, split + right.event_count());
            let slot = routes.iter().position(|route| *route == scope).unwrap();
            if selected[slot] == 0 {
                selected_events(left, start, routes, selected, events);
            } else {
                selected_events(right, split, routes, selected, events);
            }
        }
        Source::Roll(body, _) => selected_events(body, start, routes, selected, events),
    }
}

/// Print finite data, then independently check it against Lean's decoder.
/// Later facts reduce these literals instead of recomputing ownership paths.
#[cfg(not(miri))]
fn write_actor_orders<Steps: crate::g::ProgramShape>(
    images: [&'static super::RoleImageRef; 3],
    source: &mut std::string::String,
) {
    use crate::global::role_program::RoleLocalDirection;
    use std::{collections::BTreeSet, fmt::Write, string::ToString, vec::Vec};

    let program = images[0].program();
    let tree = &Steps::SOURCE_NODE;
    let mut routes = Vec::new();
    let mut route_ordinals = Vec::new();
    source_routes(tree, 0, &mut 0, &mut routes, &mut route_ordinals);
    let mut paths = std::vec![Vec::new()];
    for _ in &routes {
        paths = paths
            .into_iter()
            .flat_map(|path| {
                [0, 1].map(|arm| {
                    let mut next = path.clone();
                    next.push(arm);
                    next
                })
            })
            .collect();
    }
    let resources = (0..tree.event_count())
        .filter_map(|event| match tree.message_at(event).payload {
            crate::g::SourcePayload::Endpoint(key) => Some((key.component, key.role)),
            crate::g::SourcePayload::Wire { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let issuance = resources
        .iter()
        .map(|&(component, role)| {
            let actor = program
                .initial_actor_for(role)
                .expect("source initial owner");
            std::format!(
                "{{ endpoint := {{ component := {component}, role := {role} }}, actor := {actor} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let normalized = paths.iter().map(|path| {
        let selections = routes.iter().zip(path).map(|(&(start, split, stop), arm)| {
            std::format!("{{ scope := {{ start := {start}, split := {split}, stop := {stop} }}, arm := .{} }}",
                if *arm == 0 { "left" } else { "right" })
        }).collect::<Vec<_>>().join(", ");
        let mut events = Vec::new();
        selected_events(tree, 0, &routes, path, &mut events);
        let bindings = events.into_iter().map(|event| {
            let message = tree.message_at(event);
            let [sender, receiver] = program.actor_binding_at(event)
                .unwrap_or([u16::from(message.from.role), u16::from(message.to.role)]);
            let mut selected = |scope: crate::global::const_dsl::ScopeId| {
                let slot = route_ordinals.iter().position(|ordinal| *ordinal == scope.local_ordinal()).unwrap();
                Some(path[slot])
            };
            let sender = program.selected_actor(sender, &mut selected).unwrap();
            let receiver = program.selected_actor(receiver, &mut selected).unwrap();
            std::format!("({event}, {{ message := {}, senderActor := {sender}, receiverActor := {receiver} }})",
                source_message(tree, event))
        }).collect::<Vec<_>>().join(", ");
        std::format!("{{ selections := [{selections}], bindings := [{bindings}] }}")
    }).collect::<Vec<_>>().join(",\n  ");
    writeln!(source, "private def distributedOwnershipSource : Source := {}\n\n\
        private def distributedNormalization : Normalization := {{ issuance := [{issuance}], paths := [\n  {normalized}] }}\n\n\
        private theorem distributedOwnershipSourceDecoded : distributedChoreo.ownershipSource? = some distributedOwnershipSource := by decide\n\n\
        private theorem distributedNormalizationDecoded : normalize distributedOwnershipSource = some distributedNormalization := by decide\n",
        ownership_source(tree)).unwrap();

    let columns = program.columns;
    let bindings = (0..columns.atom_count())
        .filter_map(|event| program.actor_binding_at(event))
        .map(|[sender, receiver]| std::format!("({sender}, {receiver})"))
        .collect::<Vec<_>>()
        .join(", ");
    let decisions = (0..program.actor_decision_count())
        .map(|row| {
            let (scope, [left, right]) = program.actor_decision_at(row).unwrap();
            std::format!(
                "{{ scope := {}, left := {left}, right := {right} }}",
                scope.raw()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let initial = (0..=u8::MAX)
        .filter_map(|role| program.initial_actor_for(role).map(|actor| (role, actor)))
        .map(|(role, actor)| std::format!("({role}, {actor})"))
        .collect::<Vec<_>>()
        .join(", ");
    let scope_offset = columns.route_participants().end_offset(1)
        + if program.has_actor_bindings() { 4 } else { 0 };
    let program_u16 = |offset| {
        u16::from_le_bytes([
            program.proof_byte_at(offset),
            program.proof_byte_at(offset + 1),
        ])
    };
    let markers = (0..program.source_scope_marker_count())
        .map(|row| {
            let at = scope_offset + row * 5;
            std::format!(
                "{{ offset := {}, scope := {}, tag := {} }}",
                program_u16(at),
                program_u16(at + 2),
                program.proof_byte_at(at + 4)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(source, "private def distributedProgramColumns : DecodedProgramColumns := {{\n  \
        atomCount := {}, routeResolverCount := {}, routeParticipantCount := {},\n  \
        scopeMarkerCount := {}, scopeMarkerOffset := {scope_offset},\n  \
        actorBindings := [{bindings}], actorDecisions := [{decisions}], initialActors := [{initial}]\n}}\n\n\
        private def distributedScopeMarkers : List DecodedScopeMarker := [{markers}]\n",
        columns.atom_count(), columns.route_resolver_count(), columns.route_participant_count(),
        program.source_scope_marker_count()).unwrap();
    for image in images {
        assert!(
            image.program().same_image(program),
            "one captured global image"
        );
        let actor = image.role();
        let columns = image.columns();
        let descriptor = super::RoleDescriptorRef::from_resident(image);
        let role_u16 = |offset| {
            u16::from_le_bytes([
                descriptor.proof_byte_at(offset),
                descriptor.proof_byte_at(offset + 1),
            ])
        };
        let rows = (0..image.local_step_count()).map(|event| {
            let action = image.local_step_direction(event);
            let side = match action {
                RoleLocalDirection::Send => "send",
                RoleLocalDirection::Recv => "receive",
                RoleLocalDirection::Local => "local",
            };
            let occurrence = image.local_step_effect(event).dense_ordinal();
            let view = image.local_step_logical_role(event);
            let reference = program.actor_binding_at(occurrence).map_or(u16::from(view), |bindings| {
                bindings[usize::from(program.event_atom_at(occurrence).from != view)]
            });
            std::format!("{{ eventId := {event}, actionId := {{ event := {occurrence}, side := .{side} }}, view := {view}, actorReference := {reference} }}")
        }).collect::<Vec<_>>().join(", ");
        let lanes = (0..image.local_step_count())
            .map(|event| image.local_step_lane(event).unwrap().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let event_dependencies = (0..image.local_step_count())
            .map(|event| role_u16(columns.events.offset as usize + event * 9 + 2).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let dependencies = (0..columns.dependencies.len as usize)
            .map(|row| {
                let at = columns.dependencies.offset as usize + row * 8;
                std::format!(
                    "{{ start := {}, stop := {}, scope := {}, conflict := {} }}",
                    role_u16(at),
                    role_u16(at + 2),
                    role_u16(at + 4),
                    role_u16(at + 6)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let rolls = (0..columns.roll_scopes.len as usize)
            .map(|slot| {
                let row = image.roll_scope_row(slot).unwrap();
                std::format!(
                    "{{ scope := {}, eventStart := {}, eventLength := {} }}",
                    row.scope_raw(),
                    row.event_row().start(),
                    row.event_row().len()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(source, "private def distributedWordData{actor} : DecodedActorOrder := {{\n  \
            source := distributedOwnershipSource, normalized := distributedNormalization, actor := {actor},\n  \
            columns := distributedProgramColumns, markers := distributedScopeMarkers,\n  \
            rows := [{rows}], lanes := [{lanes}], eventDependencies := [{event_dependencies}], dependencies := [{dependencies}]\n}}\n\n\
            private theorem distributedWordData{actor}Decoded : distributedImage{actor}.decodeActorOrder? distributedChoreo = some distributedWordData{actor} := by\n  \
            simp only [RustDescriptorImage.decodeActorOrder?, distributedOwnershipSourceDecoded, Option.bind_some, distributedNormalizationDecoded] <;> rfl\n\n\
            private def distributedWordRolls{actor} : List DecodedRollRow := [{rolls}]\n\n\
            private theorem distributedWordRolls{actor}Decoded : distributedImage{actor}.decodeRollRows? = some distributedWordRolls{actor} := by decide\n").unwrap();
        writeln!(source, "private theorem distributedActorOrder{actor}Checked : distributedImage{actor}.actorOrderCheck distributedChoreo = true := by\n  \
            rw [RustDescriptorImage.actorOrderCheck, distributedWordData{actor}Decoded]\n  decide +kernel\n\n\
            private theorem distributedActorRows{actor}Checked : distributedImage{actor}.actorRowsCheck distributedChoreo = true :=\n  \
            (accepted_actor_order_binds_source_readiness distributedActorOrder{actor}Checked).1\n\n\
            private theorem distributedCanonicalRolls{actor}Decoded : distributedChoreo.canonicalActorRollRows? distributedImage{actor}.role = some distributedWordRolls{actor} := by\n  \
            simp only [Choreo.canonicalActorRollRows?, distributedOwnershipSourceDecoded, Option.bind_some, distributedNormalizationDecoded]\n  decide +kernel\n\n\
            private theorem distributedActorRoll{actor}Checked : distributedImage{actor}.actorRollCheck distributedChoreo = true := by\n  \
            simp only [RustDescriptorImage.actorRollCheck, distributedActorRows{actor}Checked, distributedCanonicalRolls{actor}Decoded, distributedWordRolls{actor}Decoded]\n  decide +kernel\n\n\
            private theorem distributedActorRoute{actor}Checked : distributedImage{actor}.actorRouteCheck distributedChoreo = true := by\n  \
            simp only [RustDescriptorImage.actorRouteCheck, distributedActorRows{actor}Checked]\n  decide +kernel\n").unwrap();
    }
}

/// Serialize observed source rows and transport headers after execution.
/// Endpoint operations do not read these records.
#[cfg(not(miri))]
pub(super) fn execution_source<Steps>(
    images: [&'static super::RoleImageRef; 3],
    initial: &[ActorSnapshot; 3],
    operations: &[OperationObservation],
    paths: &[(usize, usize)],
) -> std::string::String
where
    Steps: crate::g::ProgramShape + crate::test_support::lean_choreo::LeanChoreo,
{
    use super::RoleDescriptorRef;
    use crate::test_support::lean_descriptor::descriptor_source;
    use std::{collections::BTreeMap, fmt::Write};
    let session = operations[0].frame().header.session().raw();
    let mut source = std::format!(
        "import Hibana.DistributedSourceExecution\n\nset_option maxRecDepth 10000\n\
         set_option maxHeartbeats 0\n\nopen Hibana Hibana.EndpointDelegation\n\n\
         def distributedChoreo : Choreo := {}\n\n",
        Steps::lean_source()
    );
    for image in images {
        let actor = image.role();
        writeln!(
            source,
            "def distributedImage{actor} : RustDescriptorImage := {}\n",
            descriptor_source(RoleDescriptorRef::from_resident(image))
        )
        .unwrap();
    }
    write_actor_orders::<Steps>(images, &mut source);
    source.push_str("def distributedImages (actor : Nat) : RustDescriptorImage :=\n  if actor = 1 then distributedImage1 else if actor = 3 then distributedImage3 else distributedImage0\n\n");
    for snapshot in initial {
        writeln!(
            source,
            "def distributedInitial{} : ActorProgressObservation := {}\n",
            snapshot.actor,
            super::commit::observation_source(snapshot)
        )
        .unwrap();
    }
    source.push_str("def distributedInitialActors (actor : Nat) : ActorProgressObservation × Nat :=\n  (if actor = 1 then distributedInitial1 else if actor = 3 then distributedInitial3 else distributedInitial0, 0)\n\n\
        def distributedCut0 : DistributedSourceCut := .initial distributedInitialActors\n\n\
        def distributedEntries0 : List SourceAcceptance := []\n\n");
    let mut accepted = BTreeMap::new();
    let mut channels = BTreeMap::<_, (std::vec::Vec<(u64, u16)>, usize)>::new();
    let mut stage = 0;
    let mut records = std::vec::Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let (previous_path, next_path) = paths[index];
        let before = &operation.before;
        let after = &operation.after;
        let actor = before.actor;
        let frame = operation.frame();
        let header = frame.header;
        let event = before
            .steps
            .iter()
            .enumerate()
            .position(|(index, step)| {
                operation.after.steps[index].done
                    && step.view == operation.view
                    && match (&operation.frame, step.action) {
                        (FrameBoundary::Sent(_), super::LocalAction::Send { label, .. })
                        | (FrameBoundary::Received(_), super::LocalAction::Recv { label, .. }) => {
                            label == operation.label
                        }
                        _ => false,
                    }
            })
            .unwrap();
        writeln!(source, "def distributedBefore{index} : ActorProgressObservation := {}\n\n\
            def distributedAfter{index} : ActorProgressObservation := {}\n\n\
            def distributedWire{index} : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n\
            def distributedChannel{index} : TransportChannel := {{ session := {}, generation := 0, lane := {}, sender := {}, receiver := {} }}\n",
            super::commit::observation_source(before), super::commit::observation_source(after),
            header.session().raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(), frame.payload(),
            header.session().raw(), header.lane(), header.source_role(), header.target_role()).unwrap();
        let (base, commit, measurement) = if let Some(scope) = before.steps[event].reentry_scope {
            let image = images.iter().find(|image| image.role() == actor).unwrap();
            let roll = (0..image.columns().roll_scopes.len as usize)
                .filter_map(|slot| image.roll_scope_row(slot))
                .find(|roll| roll.scope_raw() == scope)
                .expect("observed reentry has a projected roll");
            let rows = roll.event_row();
            let mut prepared = before.clone();
            for step in &mut prepared.steps[rows.start()..rows.end()] {
                step.done = false;
            }
            for (route, selected) in &mut prepared.choices {
                let slot = image
                    .route_scope_slot(crate::global::const_dsl::ScopeId::route(*route))
                    .expect("observed route is projected");
                let mut bounds = None;
                for arm in [0, 1] {
                    let arm_rows = image.route_arm_event_row_by_slot(slot, arm);
                    if !arm_rows.is_absent_or_zero_len() {
                        bounds = Some(bounds.map_or(
                            (arm_rows.start(), arm_rows.end()),
                            |(start, stop): (usize, usize)| {
                                (start.min(arm_rows.start()), stop.max(arm_rows.end()))
                            },
                        ));
                    }
                }
                if bounds.is_some_and(|(start, stop)| rows.start() <= start && stop <= rows.end()) {
                    *selected = None;
                }
            }
            writeln!(source, "def distributedReentry{index} : ActorReentryCommitObservation := {{ previousPath := {previous_path}, nextPath := {next_path}, scope := {scope}, event := {event}, view := {}, before := distributedBefore{index}, after := distributedAfter{index} }}\n\n\
                def distributedPreparedPair{index} : DecodedRollRow × ActorCommitObservation :=\n  \
                ({{ scope := {scope}, eventStart := {}, eventLength := {} }},\n   \
                {{ pathIndex := {next_path}, event := {event}, view := {}, before := {}, after := distributedAfter{index} }})\n\n\
                private theorem distributedPrepared{index} : distributedReentry{index}.prepared? distributedImage{actor} distributedChoreo = some distributedPreparedPair{index} := by distributed_decide\n",
                operation.view, rows.start(), rows.len(), operation.view,
                super::commit::observation_source(&prepared)).unwrap();
            (
                std::format!("distributedPreparedPair{index}.2.before"),
                std::format!("distributedPreparedPair{index}.2"),
                std::format!(
                    ".fresh (sample := distributedReentry{index}) (by\n    simp only [actorReentryMutationCheck, distributedPrepared{index}]\n    exact Bool.and_eq_true_iff.mpr ⟨distributedCommit{index}Checked, by decide⟩) distributedPrepared{index}"
                ),
            )
        } else {
            (
                std::format!("distributedBefore{index}"),
                std::format!(
                    "{{ pathIndex := {next_path}, event := {event}, view := {}, before := distributedBefore{index}, after := distributedAfter{index} }}",
                    operation.view
                ),
                std::format!(
                    ".ordinary (record := {{ commit := distributedCommit{index}, before := {}, after := {} }}) distributedCommit{index}Checked (by decide)",
                    before.completion_mutations,
                    after.completion_mutations
                ),
            )
        };
        writeln!(source, "def distributedCommit{index} : ActorCommitRecord := {{ step := {commit}, wire := distributedWire{index} }}\n\n\
            private theorem distributedCommit{index}Checked : distributedCommit{index}.check distributedImage{actor} distributedChoreo {session} = true := by distributed_decide\n\n\
            private theorem distributedMeasurement{index} : ActorMeasuredCommit distributedImage{actor} distributedChoreo {session}\n  (distributedBefore{index}, {}) (distributedAfter{index}, {}) {} distributedCommit{index} :=\n  {measurement}\n",
            before.completion_mutations, after.completion_mutations,
            if before.steps[event].reentry_scope.is_some() {
                std::format!("((List.range distributedPreparedPair{index}.1.eventLength).map (distributedPreparedPair{index}.1.eventStart + ·))")
            } else { "[]".into() }).unwrap();
        let image = images.iter().find(|image| image.role() == actor).unwrap();
        let effect = image.local_step_effect(event).dense_ordinal();
        if let crate::g::SourcePayload::Endpoint(resource) =
            Steps::SOURCE_NODE.message_at(effect).payload
        {
            let side = if matches!(operation.frame, FrameBoundary::Sent(_)) {
                "send"
            } else {
                "receive"
            };
            writeln!(source, "private def distributedOwnedSource{index} : OwnedSourceFacts := {{\n  \
                effIndex := {effect}, resource := {{ component := {}, role := {} }},\n  \
                binding := ((distributedWordData{actor}.normalized.paths[{next_path}].bindings.find? (fun entry => entry.1 == {effect})).get (by decide)).2,\n  \
                side := .{side}, lane := {}, frameLabel := {}\n}}\n\n\
                private theorem distributedOwnedSource{index}Decoded : distributedImage{actor}.ownedSourceFacts? distributedChoreo {next_path} {event} = some distributedOwnedSource{index} := by distributed_decide\n\n\
                private def distributedOwnedCommit{index} : OwnedCommitFacts := {{\n  \
                effIndex := distributedOwnedSource{index}.effIndex, resource := distributedOwnedSource{index}.resource,\n  \
                binding := distributedOwnedSource{index}.binding, side := distributedOwnedSource{index}.side,\n  \
                lane := distributedOwnedSource{index}.lane, frameLabel := distributedOwnedSource{index}.frameLabel,\n  \
                afterView := (observedActorView? distributedAfter{index} {}).get (by decide)\n}}\n\n\
                private theorem distributedOwnedCommit{index}Decoded : distributedCommit{index}.step.ownedFacts? distributedImage{actor} distributedChoreo = some distributedOwnedCommit{index} := by distributed_decide\n",
                resource.component, resource.role, header.lane(), header.label().raw(), resource.role).unwrap();
        }
        // Emit finite carrier snapshots rather than recursively embedding every
        // earlier send and its Option proof. Each generated accept/receive
        // theorem still checks this snapshot against the actual transition.
        let (frames, delivered) = channels
            .entry((
                header.session().raw(),
                header.lane(),
                header.source_role(),
                header.target_role(),
            ))
            .or_default();
        match &operation.frame {
            FrameBoundary::Sent(_) => frames.push((frame.witness, header.label().raw())),
            FrameBoundary::Received(_) => {
                assert_eq!(
                    frames.get(*delivered),
                    Some(&(frame.witness, header.label().raw())),
                    "observed receive must consume the accepted FIFO head"
                );
                *delivered += 1;
            }
        }
        let frame_source = frames
            .iter()
            .enumerate()
            .map(|(sequence, (_, label))| {
                std::format!(
                    "{{ channel := distributedChannel{index}, sequence := {sequence}, frameLabel := ⟨{label}, by decide⟩ }}"
                )
            })
            .collect::<std::vec::Vec<_>>()
            .join(", ");
        let next_stage = stage + 1;
        writeln!(source, "def distributedTransport{next_stage} : TransportState := {{ channel := distributedChannel{index}, frames := [{frame_source}], delivered := {delivered}, peerClosed := false }}\n").unwrap();
        if matches!(operation.frame, FrameBoundary::Sent(_)) {
            accepted.insert(frame.witness, index);
            writeln!(source, "def distributedSend{index} : SourceSendAdmission := {{ image := distributedImage{actor}, base := {base}, pathIndex := {next_path}, event := {event}, wire := distributedWire{index} }}\n\n\
                private theorem distributedSend{index}Checked : distributedSend{index}.check distributedChoreo {session} = true := by distributed_decide\n\n\
                def distributedEntry{index} : SourceAcceptance := {{ sample := distributedSend{index}, channel := distributedChannel{index}, event := {}, mutation := {}, carrierWitness := {} }}\n\n\
                def distributedCut{next_stage} : DistributedSourceCut := distributedCut{stage}.carrierAt distributedChannel{index} distributedTransport{next_stage}\n\n\
                def distributedEntries{next_stage} := distributedEntries{stage} ++ [distributedEntry{index}]\n",
                match before.steps[event].action {
                    super::LocalAction::Send {eff_index, ..} => eff_index.dense_ordinal(),
                    super::LocalAction::Recv { .. } | super::LocalAction::Local { .. } | super::LocalAction::Terminate => unreachable!("observed send source"),
                },
                before.completion_mutations, frame.witness).unwrap();
            let preparation = if before.steps[event].reentry_scope.is_some() {
                std::format!(
                    ".fresh (rolls := distributedWordRolls{actor}) (roll := distributedPreparedPair{index}.1) distributedCanonicalRolls{actor}Decoded (by decide) (by distributed_decide)"
                )
            } else {
                ".current (by decide)".into()
            };
            writeln!(source, "private theorem distributedStep{} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{}, distributedEntries{}) none := by\n  apply DistributedSourceStep.accept (sample := distributedSend{index}) (previousPath := {previous_path}) (witness := {}) (label := ⟨{frame_label}, by decide⟩)\n    (data := distributedWordData{actor})\n    (path := distributedWordData{actor}.normalized.paths[{next_path}])\n    (row := distributedWordData{actor}.rows[{event}])\n  · rfl\n  · exact distributedSend{index}Checked\n  · exact distributedWordData{actor}Decoded\n  · exact getElem?_pos _ _ (by decide)\n  · exact getElem?_pos _ _ (by decide)\n  · exact {preparation}\n  · decide\n  · decide\n  · decide\n  · decide\n  · unfold OwnedWireObservation.identifies; decide\n",
                stage+1,stage+1,stage+1,frame.witness, frame_label = header.label().raw()).unwrap();
            stage += 1;
            writeln!(source, "def distributedCut{} : DistributedSourceCut := distributedCut{stage}.actorAt {actor} (distributedAfter{index}, {})\n\n\
                def distributedEntries{} := distributedEntries{stage}\n\n\
                private theorem distributedStep{} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{}, distributedEntries{}) (some ({actor}, distributedCommit{index})) := by\n  exact .publish (entry := distributedEntry{index}) (List.mem_append_right _ (List.mem_singleton_self _)) (by decide) (by\n    simp only [sourcePublicationPairCheck, distributedSend{index}Checked, distributedCommit{index}Checked, sourceOperationIdentity?, distributedWordData{actor}Decoded]\n    decide +kernel) distributedMeasurement{index}\n",
                stage+1,after.completion_mutations,stage+1,stage+1,stage+1,stage+1).unwrap();
        } else {
            let sent_index = accepted[&frame.witness];
            let sender_actor = operations[sent_index].before.actor;
            writeln!(source, "def distributedCut{next_stage} : DistributedSourceCut := (distributedCut{stage}.actorAt {actor} (distributedAfter{index}, {})).carrierAt distributedChannel{index} distributedTransport{next_stage}\n\n\
                def distributedEntries{next_stage} := distributedEntries{stage}\n\n\
                private theorem distributedStep{next_stage} : DistributedSourceStep distributedImages distributedChoreo {session}\n  (distributedCut{stage}, distributedEntries{stage}) (distributedCut{next_stage}, distributedEntries{next_stage}) (some ({actor}, distributedCommit{index})) := by\n  exact .receive (frame := ((distributedCut{stage}.carriers distributedChannel{index}).frames[(distributedCut{stage}.carriers distributedChannel{index}).delivered]?).get (by decide)) (entry := distributedEntry{sent_index}) (witness := {}) (by decide) (by decide) (by rfl) (by decide) (a_concrete_receive_pair_has_coherent_local_witnesses (by\n    simp only [sourceReceivePairCheck, distributedSend{sent_index}Checked, distributedCommit{index}Checked, sourceOperationIdentity?, distributedWordData{sender_actor}Decoded, distributedWordData{actor}Decoded]\n    decide +kernel)) (by decide) distributedMeasurement{index}\n",
                after.completion_mutations,frame.witness).unwrap();
        }
        stage += 1;
        records.push(std::format!("({actor}, distributedCommit{index})"));
    }
    writeln!(source, "def distributedRecords : List (Nat × ActorCommitRecord) := [{}]\n\n\
        theorem distributedActualExecution : DistributedSourceExecution distributedImages distributedChoreo {session}\n  (distributedCut0, distributedEntries0) (distributedCut{stage}, distributedEntries{stage}) distributedRecords := by\n  exact {}\n\n\
        theorem distributedActorHistories (actor : Nat) : ∃ cleared commits, ActorMeasuredHistory (distributedImages actor) distributedChoreo {session}\n  (distributedCut0.actors actor) (distributedCut{stage}.actors actor) cleared commits :=\n  a_distributed_source_execution_projects_each_actual_actor_history distributedActualExecution actor\n\n\
        theorem distributedFifoHistories (channel : TransportChannel) : ∃ history, SendAdmissionTrace distributedChoreo {session}\n  (distributedCut{stage}.carriers channel) history (sourceChannelRecords distributedEntries{stage} channel) :=\n  a_distributed_source_execution_projects_its_complete_fifo_histories distributedActualExecution channel\n",
        records.join(", "), (1..=stage).fold(".refl".into(), |proof, boundary| std::format!("(.tail {proof} distributedStep{boundary})"))).unwrap();
    write_frame_words(initial, &mut source);
    let (facts, observations) = source.split_once("def distributedImages").unwrap();
    std::format!(
        "{facts}def distributedImages{}",
        observations
            .replace("by decide", "by distributed_decide")
            .replace("· decide", "· distributed_decide")
            .replace("; decide", "; distributed_decide")
            .replace("distributed_decide", DISTRIBUTED_DECIDE)
    )
}

/// Derive every remote row's complete wire class from the captured image.
/// No row/selector is chosen to make the source-word check pass. This only
/// serializes proof data after the Endpoint execution has completed.
#[cfg(not(miri))]
fn write_frame_words(initial: &[ActorSnapshot; 3], source: &mut std::string::String) {
    use std::fmt::Write;
    source.insert_str(0, "import Hibana.SourceFrameWord\n");
    for snapshot in initial {
        let actor = snapshot.actor;
        for (event, step) in snapshot.steps.iter().enumerate() {
            if matches!(step.action, super::LocalAction::Local { .. }) {
                continue;
            }
            let name = std::format!("distributedFrameClass{actor}_{event}");
            writeln!(source, "private def distributedFrameKey{actor}_{event} := (distributedImage{actor}.frameKey? {event}).get (by decide)\n\n\
              private def {name} : SourceOrderGroup distributedImage{actor} distributedChoreo := {{\n\
              rows := distributedImage{actor}.frameClassRows distributedFrameKey{actor}_{event}\n\
              distinct := by decide\n\
              data := distributedWordData{actor}\n\
              rolls := distributedWordRolls{actor}\n\
              choices := (distributedImage{actor}.frameClassChoices? distributedChoreo\n\
                distributedFrameKey{actor}_{event}).get (by decide)\n\
              decoded := distributedWordData{actor}Decoded\n\
              decodedRolls := distributedWordRolls{actor}Decoded\n\
              nonempty := by decide\n\
              paths := by decide\n\
              choiceMembers := by decide\n\
              slots := by decide\n\
              active := by decide\n\
              ordered := by decide\n\
              uniformReset := by decide\n\
              outsideChoices := by decide\n\
            }}\n").unwrap();
        }
        writeln!(source, r#"theorem distributedActor{actor}AllFrameClassesFollowTheirSourceWords
    (event : Nat) (bounded : event ∈ List.range distributedImage{actor}.eventCount)
    (key : SourceFrameKey) (keyAt : distributedImage{actor}.frameKey? event = some key) :
    sourceFrameWord (distributedImage{actor}.frameClassRows key)
      (committedFrameClassWord (distributedImage{actor}.frameClassRows key) {actor} distributedRecords) := by
  have count : distributedImage{actor}.eventCount = {} := by decide
  have bound := List.mem_range.mp bounded
  rw [count] at bound
  have alternatives : {} := by omega
  rcases alternatives with {}"#,
            snapshot.steps.len(),
            (0..snapshot.steps.len()).map(|event| std::format!("event = {event}")).collect::<std::vec::Vec<_>>().join(" ∨ "),
            (0..snapshot.steps.len()).map(|event| std::format!("at{event}")).collect::<std::vec::Vec<_>>().join(" | ")).unwrap();
        for (event, step) in snapshot.steps.iter().enumerate() {
            writeln!(source, "  · subst event").unwrap();
            if matches!(step.action, super::LocalAction::Local { .. }) {
                writeln!(source, "    have absent : distributedImage{actor}.frameKey? {event} = none := by decide\n    rw [absent] at keyAt\n    cases keyAt").unwrap();
            } else {
                writeln!(source, r#"    have actual : distributedImage{actor}.frameKey? {event} = some distributedFrameKey{actor}_{event} := by decide
    have same := Option.some.inj (keyAt.symm.trans actual)
    subst key
    exact (a_measured_distributed_class_has_its_source_word_and_exact_prefix
      (actor := {actor}) (group := distributedFrameClass{actor}_{event}) distributedActualExecution (by decide)).1"#).unwrap();
            }
        }
    }
}

#[cfg(not(miri))]
pub(super) fn write_observations(
    images: [&'static super::RoleImageRef; 3],
    initial: &[ActorSnapshot; 3],
    operations: &[OperationObservation],
    candidates: &[(ActorSnapshot, std::vec::Vec<bool>)],
) {
    let mut source = execution_source::<super::mixed_send_admission::Steps>(
        images,
        initial,
        operations,
        &std::vec![(0, 0); operations.len()],
    );
    source.insert_str(
        0,
        "import Hibana.FifoOccurrenceOrigin\nimport Hibana.SourcePublicationPrefix\nimport Hibana.SourceOwnedReceive\nimport Hibana.OccurrenceResourceOrigin\nimport Hibana.ResourceOccurrenceCausality\nimport Hibana.OccurrenceResourceCustody\n",
    );
    use std::fmt::Write;
    writeln!(source, "private theorem distributedBeforeFreshReceiptExecution : DistributedSourceExecution distributedImages distributedChoreo 9062\n  (distributedCut0, distributedEntries0) (distributedCut13, distributedEntries13) (distributedRecords.take 8) := by\n  exact {}\n",
        (1..=13).fold(".refl".into(), |proof, boundary| std::format!("(.tail {proof} distributedStep{boundary})"))).unwrap();
    source.push_str(r#"
theorem distributedMixedCutKeepsIndependentActorVisits :
    (distributedCut13.actors 0).2 = 9 ∧
    (distributedCut13.actors 1).2 = 3 ∧
    (distributedCut13.carriers distributedChannel8).delivered = 2 ∧
    (distributedCut13.carriers distributedChannel8).frames.length = 4 := by decide

theorem distributedRepeatedLoanHasIdenticalBytes :
    distributedWire0 = distributedWire6 ∧ distributedWire6 = distributedWire8 := by decide

theorem distributedFreshReceiptAlsoFitsOldBytes :
    sourceReceivePairCheck distributedSend0 distributedCommit8 distributedImage1 distributedChoreo 9062 = true := by decide

theorem distributedFreshReceiptHasItsActualFifoHead :
    (sourceChannelEntries distributedEntries13 distributedChannel8)[
      (distributedCut13.carriers distributedChannel8).delivered]? = some distributedEntry6 := by decide

theorem distributedOldWitnessCannotIdentifyTheFreshFifoHead
    (entry : SourceAcceptance)
    (head : (sourceChannelEntries distributedEntries13 distributedChannel8)[
      (distributedCut13.carriers distributedChannel8).delivered]? = some entry) :
    entry.carrierWitness ≠ distributedEntry0.carrierWitness := by
  have identical := Option.some.inj (distributedFreshReceiptHasItsActualFifoHead.symm.trans head)
  subst entry
  decide

theorem distributedFreshReceiptResetsEvenWhenItsRowBitStaysTrue :
    distributedBefore8.doneState.done 0 = true ∧
    distributedAfter8.doneState.done 0 = true ∧
    distributedBefore8.progress ≠ distributedAfter8.progress ∧
    distributedPreparedPair8.2.before.doneState.done 0 = false := by decide

theorem distributedCurrentReceiptHasItsActualUnresetGlobalOrigin :
    ∃ before after prior later record marked untouched commits,
      DistributedSourceExecution distributedImages distributedChoreo 9062
        (distributedCut0, distributedEntries0) before prior ∧
      DistributedSourceStep distributedImages distributedChoreo 9062 before after (some (1, record)) ∧
      ActorMeasuredCommit distributedImage1 distributedChoreo 9062
        (before.1.actors 1) (after.1.actors 1) marked record ∧
      DistributedSourceExecution distributedImages distributedChoreo 9062
        after (distributedCut24, distributedEntries24) later ∧
      ActorMeasuredHistory distributedImage1 distributedChoreo 9062
        (after.1.actors 1) (distributedCut24.actors 1) untouched commits ∧
      0 ∉ untouched ∧ record.step.event = 0 :=
  a_current_distributed_row_has_its_unreset_actual_boundary distributedActualExecution (by decide) (by decide)

theorem distributedActualReceiveBoundaries
    (actor eff : Nat) (record : ActorCommitRecord) (binding : Binding)
    (member : (actor, record) ∈ distributedRecords)
    (receiveAt : sourceOperationIdentity? (distributedImages actor) distributedChoreo
      record.step.pathIndex record.step.event = some (eff, binding, .receive)) :
    ∃ before after prior later channel entry witness next frame,
      DistributedSourceExecution distributedImages distributedChoreo 9062
        (distributedCut0, distributedEntries0) before prior ∧
      DistributedSourceStep distributedImages distributedChoreo 9062 before after (some (actor, record)) ∧
      (before.1.carriers channel).pollReceive = .delivered frame next ∧
      (sourceChannelEntries before.2 channel)[(before.1.carriers channel).delivered]? = some entry ∧
      entry.carrierWitness = witness ∧
      SourceReceivePair entry.sample record (distributedImages actor) distributedChoreo 9062 ∧
      DistributedSourceExecution distributedImages distributedChoreo 9062
        after (distributedCut24, distributedEntries24) later :=
  a_logged_receive_has_its_exact_fifo_acceptance distributedActualExecution member receiveAt

def distributedFreshSentFacts := (distributedEntry6.sample.ownedFacts? distributedChoreo).get (by decide)
def distributedFreshReceivedFacts := (distributedCommit8.step.ownedFacts? distributedImage1 distributedChoreo).get (by decide)

theorem distributedProspectiveReceiptHasItsSourceInstanceBeforePairValidation :
    ∃ history key following,
      SendAdmissionTrace distributedChoreo 9062 (distributedCut13.carriers distributedChannel8) history
        (sourceChannelRecords distributedEntries13 distributedChannel8) ∧
      history.accepted[(distributedCut13.carriers distributedChannel8).delivered]? = some key ∧
      key.globalId = distributedEntry6.event ∧
      key.iteration = sourceOccurrenceOrdinal
        (sourceChannelRecords distributedEntries13 distributedChannel8)
        (distributedCut13.carriers distributedChannel8).delivered distributedEntry6.event ∧
      distributedEntry6.sample.effIndex? = some distributedEntry6.event ∧
      distributedEntry6.sample.wire.identifies
        (((distributedCut13.carriers distributedChannel8).frames[2]?).get (by decide)) ∧
      distributedFreshSentFacts.resource = distributedFreshReceivedFacts.resource ∧
      distributedFreshSentFacts.effIndex = distributedFreshReceivedFacts.effIndex ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace distributedChoreo 9062 distributedTransport14 following
        (sourceChannelRecords distributedEntries13 distributedChannel8) :=
  a_prospective_owned_receipt_has_its_distributed_fifo_source_instance
    distributedBeforeFreshReceiptExecution (by decide) distributedFreshReceiptHasItsActualFifoHead
    ((Option.some_get (x := distributedEntry6.sample.ownedFacts? distributedChoreo) (by decide)).symm)
    (by decide)
    ((Option.some_get (x := distributedCommit8.step.ownedFacts? distributedImage1 distributedChoreo) (by decide)).symm)
    rfl

theorem distributedFirstAcceptedSendHasItsUnpublishedPrefix :
    ∃ pending,
      actorAcceptedSends distributedEntries1 0 = pending ∧ pending.length = 1 ∧
      ∀ entry ∈ pending, entry.stamps 0 (distributedCut1.actors 0).2 := by
  obtain ⟨pairs, pending, accepted, published, _, _, stamped⟩ :=
    a_distributed_actors_acceptances_lead_publications_by_at_most_one
      (actor := 0) (DistributedSourceExecution.tail .refl distributedStep1) rfl
  have lengths := congrArg List.length published
  have empty : pairs = [] := List.eq_nil_of_length_eq_zero (by
    simpa [actorPublishedSends] using lengths.symm)
  have equal : actorAcceptedSends distributedEntries1 0 = pending := by
    simpa only [empty, List.map_nil, List.nil_append] using accepted
  have count : (actorAcceptedSends distributedEntries1 0).length = 1 := by decide
  rw [equal] at count
  exact ⟨pending, equal, count, stamped⟩

private theorem distributedFreshFrameSourceIdentity :
    distributedEntry6.sample.effIndex? = distributedImage1.eventEffIndex? 0 := by
  let wire := distributedEntry6.sample.wire
  have sender := decoded_frame_key_retains_its_source_position
    (image := distributedEntry6.sample.image) (row := distributedEntry6.sample.event)
    (key := ⟨.send, wire.sender, wire.receiver, wire.lane, wire.frameLabel⟩) (by decide)
  have receiver := decoded_frame_key_retains_its_source_position
    (image := distributedImage1) (row := 0)
    (key := ⟨.receive, wire.receiver, wire.sender, wire.lane, wire.frameLabel⟩) (by decide)
  exact sender.trans receiver.symm

theorem distributedFreshMixedSourceIdentityBeforeItsNewReceivePair :
    distributedEntry6.sample.effIndex? = distributedImage1.eventEffIndex? distributedCommit8.step.event :=
  distributedFreshFrameSourceIdentity
"#);
    source.push_str(include_str!("distributed_owned_receive.lean"));
    source.push_str("\nprivate def distributedCandidateData := distributedWordData0\n\n\
        private def distributedCandidatePath := (distributedCandidateData.normalized.paths[0]?).get (by decide)\n\n\
        private def distributedCandidateRolls := (distributedChoreo.canonicalActorRollRows? 0).get (by decide)\n\n\
        private def distributedCandidateRoll := (distributedCandidateRolls[0]?).get (by decide)\n\n");
    for (index, (before, admitted)) in candidates.iter().enumerate() {
        let label = if index % 2 == 0 { 1 } else { 9 };
        let event = before
            .steps
            .iter()
            .position(|step| matches!(step.action, super::LocalAction::Send { label: found, .. } if found == label))
            .unwrap();
        writeln!(source, "private def distributedCandidate{index} : ActorAdmissionObservation := {{ snapshot := {}, pathIndex := 0, admitted := {admitted:?}, retainedRolls := {:?} }}\n\n\
            private def distributedCandidateRow{index} := (distributedCandidateData.rows[{event}]?).get (by decide)\n",
            super::commit::observation_source(before), before.retained_rolls).unwrap();
        let base = if let Some(scope) = before.steps[event].reentry_scope {
            std::format!(
                "((prepareObservedActorBody? distributedImage0 distributedChoreo distributedCandidate{index}.snapshot 0 0 {scope} {event}).get (by decide)).2"
            )
        } else {
            std::format!("distributedCandidate{index}.snapshot")
        };
        writeln!(
            source,
            "private def distributedCandidateBase{index} : ActorProgressObservation := {base}\n"
        )
        .unwrap();
    }
    source.push_str(include_str!("distributed_occurrence.lean"));
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("DistributedSourceGenerated.lean"), source).unwrap();
}
