use super::{EndpointKey, ErrorKind, Message, Payload, Plan, Scratch, Source, compile_into};

const P: EndpointKey = EndpointKey {
    component: 0,
    role: 0,
};
const DB: EndpointKey = EndpointKey {
    component: 0,
    role: 10,
};
const AUTH: EndpointKey = EndpointKey {
    component: 0,
    role: 11,
};

fn actor() -> EndpointKey {
    let role: u8 = kani::any();
    kani::assume((1..=3).contains(&role));
    EndpointKey { component: 0, role }
}

fn transfer(from: EndpointKey, to: EndpointKey, key: EndpointKey) -> Source<'static> {
    Source::Send(Message {
        from,
        to,
        label: 1,
        payload: Payload::Endpoint(key),
    })
}

#[kani::proof]
#[kani::unwind(8)]
fn independent_resources_keep_distinct_affine_bindings() {
    let a = actor();
    let b = actor();
    let left = transfer(P, a, DB);
    let right = transfer(P, b, AUTH);
    let source = Source::Par(&left, &right);
    let mut scratch = Scratch::<6, 2>::new();
    let mut output = Plan::new();
    assert!(compile_into(&source, &mut scratch, &mut output).is_ok());
    assert_eq!(output.binding(0).unwrap().receiver_actor, a.role);
    assert_eq!(output.binding(1).unwrap().receiver_actor, b.role);
    assert_eq!(output.issuance(0).unwrap().endpoint, DB);
    assert_eq!(output.issuance(1).unwrap().endpoint, AUTH);
}

#[kani::proof]
#[kani::unwind(8)]
fn parallel_double_grant_never_publishes_a_plan() {
    let a = actor();
    let b = actor();
    let left = transfer(P, a, DB);
    let right = transfer(P, b, DB);
    let source = Source::Par(&left, &right);
    let mut scratch = Scratch::<6, 2>::new();
    let mut output = Plan::new();
    assert_eq!(
        compile_into(&source, &mut scratch, &mut output)
            .unwrap_err()
            .kind,
        ErrorKind::ParallelOwnershipConflict
    );
    assert!(output.is_empty());
    assert!(output.binding(0).is_none());
    assert!(output.issuance(0).is_none());
}

#[kani::proof]
#[kani::unwind(8)]
fn a_different_holder_cannot_return_the_endpoint() {
    let a = actor();
    let b = actor();
    let give = transfer(P, a, DB);
    let give_back = transfer(b, P, DB);
    let source = Source::Seq(&give, &give_back);
    let mut scratch = Scratch::<6, 2>::new();
    let mut output = Plan::new();
    let result = compile_into(&source, &mut scratch, &mut output);
    if a.role == b.role {
        assert!(result.is_ok());
    } else {
        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::ConflictingInitialOwners
        );
        assert!(output.is_empty());
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn repetition_cannot_reissue_an_unreturned_endpoint() {
    let a = actor();
    let give = transfer(P, a, DB);
    let source = Source::Roll(&give);
    let mut scratch = Scratch::<3, 1>::new();
    let mut output = Plan::new();
    assert_eq!(
        compile_into(&source, &mut scratch, &mut output)
            .unwrap_err()
            .kind,
        ErrorKind::RollChangesOwner
    );
    assert!(output.is_empty());
}

#[kani::proof]
#[kani::unwind(8)]
fn owned_operation_preserves_its_logical_wire_identity() {
    let a = actor();
    let label: u8 = kani::any();
    let schema: u32 = kani::any();
    let server = EndpointKey {
        component: 7,
        role: 4,
    };
    let give = transfer(P, a, DB);
    let message = Message {
        from: DB,
        to: server,
        label,
        payload: Payload::Wire { schema },
    };
    let work = Source::Send(message);
    let source = Source::Seq(&give, &work);
    let mut scratch = Scratch::<6, 2>::new();
    let mut output = Plan::new();
    assert!(compile_into(&source, &mut scratch, &mut output).is_ok());
    let binding = output.binding(1).unwrap();
    assert_eq!(binding.message, message);
    assert_eq!(binding.sender_actor, a.role);
    assert_eq!(binding.receiver_actor, server.role);
}

#[kani::proof]
#[kani::unwind(8)]
fn terminal_choice_keeps_branch_local_holders_without_a_global_owner_guess() {
    let a = actor();
    let b = actor();
    let left = transfer(P, a, DB);
    let right = transfer(P, b, DB);
    let source = Source::Route {
        left: &left,
        right: &right,
        resolver: None,
    };
    let mut scratch = Scratch::<6, 2>::new();
    let mut output = Plan::new();
    assert!(compile_into(&source, &mut scratch, &mut output).is_ok());
    assert_eq!(output.binding(0).unwrap().receiver_actor, a.role);
    assert_eq!(output.binding(1).unwrap().receiver_actor, b.role);
    assert_eq!(output.issuance(0).unwrap().actor, P.role);
}
