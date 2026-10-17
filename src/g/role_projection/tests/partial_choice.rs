//! A nested global determines the holder without a mutable ownership table.
use super::*;

type FirstNotice = g::Msg<5, ()>;
type SecondNotice = g::Msg<6, ()>;
type ThirdNotice = g::Msg<7, ()>;

pub(super) type Steps = g::Seq<
    g::Route<
        g::Route<
            g::Seq<
                g::Send<0, 1, Loan>,
                g::Seq<g::Send<0, 2, FirstNotice>, g::Send<0, 3, FirstNotice>>,
            >,
            g::Seq<
                g::Send<0, 1, SecondNotice>,
                g::Seq<g::Send<0, 2, Loan>, g::Send<0, 3, SecondNotice>>,
            >,
        >,
        g::Seq<g::Send<0, 1, ThirdNotice>, g::Seq<g::Send<0, 2, ThirdNotice>, g::Send<0, 3, Loan>>>,
    >,
    g::Seq<g::Send<10, 4, Read>, g::Send<4, 10, Value>>,
>;

// Source-level outcomes: the outer right arm lends DB to actor 3. Only the
// outer left arm needs the inner choice, which lends DB to actor 1 or actor 2.
// These include incomplete evidence; they are not another execution cursor.
pub(super) const CASES: [(Option<u8>, Option<u8>, Option<u8>); 9] = [
    (None, None, None),
    (None, Some(0), None),
    (None, Some(1), None),
    (Some(0), None, None),
    (Some(0), Some(0), Some(1)),
    (Some(0), Some(1), Some(2)),
    (Some(1), None, Some(3)),
    (Some(1), Some(0), Some(3)),
    (Some(1), Some(1), Some(3)),
];

#[test]
fn partial_choice_evidence_never_guesses_or_changes_a_known_holder() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let program = g::ProgramProjection::<Steps, 32>::PROGRAM_REF;
    let [reference, service] = program
        .actor_binding_at(program.columns.atom_count() - 2)
        .unwrap();
    assert_eq!(service, 4);
    let (outer_scope, [inner_reference, right_actor]) = program
        .actor_decision_at(usize::from(reference) - 256)
        .unwrap();
    let (inner_scope, inner_actors) = program
        .actor_decision_at(usize::from(inner_reference) - 256)
        .unwrap();
    assert_eq!(right_actor, 3);
    assert_eq!(inner_actors, [1, 2]);
    assert_ne!(outer_scope, inner_scope);

    for (outer, inner, expected) in CASES {
        let observed = program.selected_actor(reference, &mut |scope| {
            if scope == outer_scope {
                outer
            } else {
                assert_eq!(scope, inner_scope);
                inner
            }
        });
        assert_eq!(observed, expected, "outer={outer:?} inner={inner:?}");

        let Some(actor) = observed else { continue };
        for (extended_outer, extended_inner, _) in CASES {
            if outer.is_some() && outer != extended_outer
                || inner.is_some() && inner != extended_inner
            {
                continue;
            }
            assert_eq!(
                program.selected_actor(reference, &mut |scope| {
                    if scope == outer_scope {
                        extended_outer
                    } else {
                        assert_eq!(scope, inner_scope);
                        extended_inner
                    }
                }),
                Some(actor),
                "known=({outer:?}, {inner:?}) extended=({extended_outer:?}, {extended_inner:?})",
            );
        }
    }
}

#[test]
fn conditional_resource_admission_requires_a_known_local_holder() {
    use crate::global::typestate::EventAdmissionError;

    let images = [
        (1, &RoleProjection::<1, Steps, 32>::IMAGE_REF),
        (2, &RoleProjection::<2, Steps, 32>::IMAGE_REF),
        (3, &RoleProjection::<3, Steps, 32>::IMAGE_REF),
    ];
    let program = g::ProgramProjection::<Steps, 32>::PROGRAM_REF;
    let [reference, _] = program
        .actor_binding_at(program.columns.atom_count() - 2)
        .unwrap();
    let (outer_scope, [inner_reference, _]) = program
        .actor_decision_at(usize::from(reference) - 256)
        .unwrap();
    let (inner_scope, _) = program
        .actor_decision_at(usize::from(inner_reference) - 256)
        .unwrap();

    for (actor, image) in images {
        with_cursor(image, |cursor| {
            let read = (0..image.local_step_count())
                .find(|&idx| {
                    cursor.logical_role_at(idx) == Some(10)
                        && image.local_step_node(idx).unwrap().action().is_send()
                })
                .unwrap();
            for (outer, inner, expected) in CASES {
                let result = cursor.event_enabled(read, event(image, read), &mut |scope, _| {
                    if scope == outer_scope {
                        outer
                    } else {
                        assert_eq!(scope, inner_scope);
                        inner
                    }
                });
                match expected {
                    None => assert!(matches!(
                        result,
                        Err(EventAdmissionError::PredecessorPending)
                    )),
                    Some(holder) if holder != actor => {
                        assert!(matches!(result, Err(EventAdmissionError::Invariant)));
                    }
                    Some(_) => {
                        // Correct ownership alone does not skip the preceding
                        // loan receive in this actor's shared progress.
                        assert!(matches!(
                            result,
                            Err(EventAdmissionError::PredecessorPending
                                | EventAdmissionError::OtherViewPending)
                        ));
                    }
                }
            }
        });
    }
}
