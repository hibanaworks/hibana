use super::super::{EndpointKind, Message, Owner, Payload, Scratch, Source};
use super::{Atom, EndpointKey, Query};

#[kani::proof]
#[kani::unwind(5)]
fn collected_resource_catalog_contains_each_transferred_complete_key() {
    let first = EndpointKey {
        component: kani::any(),
        role: kani::any(),
    };
    let second = EndpointKey {
        component: kani::any(),
        role: kani::any(),
    };
    let query = EndpointKey {
        component: kani::any(),
        role: kani::any(),
    };
    let owner = EndpointKey {
        component: 0,
        role: 0,
    };
    let worker = EndpointKey {
        component: 0,
        role: 1,
    };
    // First use and first transfer have deliberately different orders.
    let control = Source::Send(Message {
        from: first,
        to: owner,
        label: 1,
        payload: Payload::Wire { schema: 0 },
    });
    let transfer_second = Source::Send(Message {
        from: owner,
        to: worker,
        label: 2,
        payload: Payload::Endpoint(second),
    });
    let transfer_first = Source::Send(Message {
        from: owner,
        to: worker,
        label: 3,
        payload: Payload::Endpoint(first),
    });
    let transfers = Source::seq(&transfer_second, &transfer_first);
    let source = Source::seq(&control, &transfers);
    let mut scratch = Scratch::<4, 0>::new();
    assert!(scratch.collect(&source).is_ok());
    let mut found = 0;
    for index in 0..scratch.symbol_len {
        let symbol = scratch.symbols[index].unwrap();
        if matches!(symbol.kind, EndpointKind::Delegated) && symbol.key.same(query) {
            found += 1;
        }
    }
    assert_eq!(found, usize::from(query.same(first) || query.same(second)));
    let ownership = Query::new(
        &source,
        scratch.symbols.split_at(scratch.symbol_len).0,
        &scratch.initial,
    );
    assert_eq!(ownership.is_delegated(query), found == 1);
}

#[kani::proof]
#[kani::unwind(4)]
fn normalization_reuses_only_established_complete_key_equations() {
    fn key() -> EndpointKey {
        EndpointKey {
            component: kani::any(),
            role: kani::any(),
        }
    }
    let first = key();
    let second = key();
    let other = key();
    let actor: u8 = kani::any();
    let mut scratch = Scratch::<3, 0>::new();
    assert!(scratch.collect_key(first, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(second, EndpointKind::Delegated).is_ok());
    assert!(scratch.collect_key(other, EndpointKind::Delegated).is_ok());
    scratch.initialize_owners();
    let first_index = scratch.index(first).unwrap() as u16;
    let second_index = scratch.index(second).unwrap() as u16;
    assert!(scratch.unify(Owner::Initial(first_index), Owner::Initial(second_index)));
    assert!(scratch.unify(Owner::Initial(second_index), Owner::Actor(actor)));
    let source = super::super::Source::Send(super::super::Message {
        from: first,
        to: second,
        label: 1,
        payload: super::super::Payload::Wire { schema: 0 },
    });
    let query = Query::new(&source, &scratch.symbols, &scratch.initial);
    assert!(matches!(query.normalize(Atom::Initial(first)),Atom::Actor(found) if found==actor));
    assert!(matches!(query.normalize(Atom::Initial(second)),Atom::Actor(found) if found==actor));
    if other.same(first) || other.same(second) {
        assert!(matches!(query.normalize(Atom::Initial(other)),Atom::Actor(found) if found==actor));
    } else {
        assert!(
            matches!(query.normalize(Atom::Initial(other)),Atom::Initial(found) if found.same(other))
        );
    }
    assert!(matches!(query.normalize(Atom::Actor(actor)),Atom::Actor(found) if found==actor));
}
