use super::{
    DecisionArm, ResolverBucket, ResolverError, ResolverRef, ResolverRegistrationKey,
    bucket::ResolverBucketEntry,
};
use crate::{
    g::{self, Msg},
    global::{
        compiled::images::CompiledProgramRef,
        role_program::{RoleProgram, project},
    },
    rendezvous::core::Sidecar,
};
use core::mem::MaybeUninit;

const FIRST_RESOLVER: u16 = 801;
const SECOND_RESOLVER: u16 = 802;

static LEFT: DecisionArm = DecisionArm::Left;
static RIGHT: DecisionArm = DecisionArm::Right;

fn choose_arm(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}

fn program<const LOGICAL_LABEL: u8>() -> &'static CompiledProgramRef {
    let source = g::send::<0, 1, Msg<LOGICAL_LABEL, ()>>();
    let role: RoleProgram<0> = project(&source);
    role.role_image_ref().program
}

#[kani::proof]
fn resolver_registration_key_is_program_and_id() {
    let left_id: u16 = kani::any();
    let right_id: u16 = kani::any();
    let intrinsic = crate::global::const_dsl::INTRINSIC_ROUTE_RESOLVER_ID;
    let valid = left_id != intrinsic && right_id != intrinsic;

    kani::cover!(valid && left_id == right_id);
    kani::cover!(valid && left_id != right_id);
    if valid {
        let first_program = program::<1>();
        let second_program = program::<2>();
        let left = ResolverRegistrationKey::new(first_program, left_id);
        let same_program = ResolverRegistrationKey::new(first_program, right_id);
        let other_program = ResolverRegistrationKey::new(second_program, left_id);

        assert!((left == same_program) == (left_id == right_id));
        assert!(left != other_program);
    }
}

#[kani::proof]
#[kani::should_panic]
fn resolver_registration_key_rejects_intrinsic_id() {
    let _ = ResolverRegistrationKey::new(
        program::<1>(),
        crate::global::const_dsl::INTRINSIC_ROUTE_RESOLVER_ID,
    );
}

#[kani::proof]
fn resolver_initial_storage_is_initialized_and_dispatchable() {
    let key = ResolverRegistrationKey::new(program::<1>(), FIRST_RESOLVER);
    let mut initial_slots = [MaybeUninit::<Option<ResolverBucketEntry<'static>>>::uninit(); 2];
    let initial: Sidecar<Option<ResolverBucketEntry<'static>>> = Sidecar::from_raw_parts(
        initial_slots.as_mut_ptr().cast(),
        ResolverBucket::storage_bytes(initial_slots.len()),
    );
    let mut bucket: ResolverBucket<'static> = ResolverBucket::empty();
    /* SAFETY: `initial_slots` is aligned for resolver entries, remains live for
    the harness, and is exclusively owned while `replace_storage` initializes
    every slot before publishing the sidecar through `bucket`. */
    unsafe {
        bucket.replace_storage(initial.cast(), initial_slots.len());
    }

    assert!(bucket.capacity() == initial_slots.len());
    assert!(bucket.entry_count() == 0);
    assert!(bucket.get(key).is_none());
    assert!(
        bucket
            .insert(
                key,
                ResolverRef::<FIRST_RESOLVER>::decision_state(&LEFT, choose_arm).erase(),
            )
            .is_ok()
    );
    let resolver = match bucket.get(key) {
        Some(resolver) => resolver,
        None => crate::invariant(),
    };
    assert!(matches!(resolver.resolve_decision(), Ok(DecisionArm::Left)));
}

#[kani::proof]
fn resolver_replacement_compacts_entries_and_preserves_dispatch() {
    let first_key = ResolverRegistrationKey::new(program::<1>(), FIRST_RESOLVER);
    let second_key = ResolverRegistrationKey::new(program::<2>(), SECOND_RESOLVER);
    let mut source_slots = [None; 3];
    let source = Sidecar::from_raw_parts(
        source_slots.as_mut_ptr(),
        ResolverBucket::storage_bytes(source_slots.len()),
    );
    let mut bucket: ResolverBucket<'static> = ResolverBucket::empty();
    /* SAFETY: `source_slots` is a live, aligned, exclusively owned entry array;
    `bind_from_storage` initializes its exact three-slot range before the bucket
    can read it. */
    unsafe {
        bucket.bind_from_storage(source, source_slots.len());
    }
    assert!(
        bucket
            .insert(
                first_key,
                ResolverRef::<FIRST_RESOLVER>::decision_state(&LEFT, choose_arm).erase(),
            )
            .is_ok()
    );
    assert!(
        bucket
            .insert(
                second_key,
                ResolverRef::<SECOND_RESOLVER>::decision_state(&RIGHT, choose_arm).erase(),
            )
            .is_ok()
    );
    let hole = usize::from(kani::any::<u8>() % 3);
    let first_entry = source_slots[0];
    let second_entry = source_slots[1];
    source_slots = match hole {
        0 => [None, first_entry, second_entry],
        1 => [first_entry, None, second_entry],
        2 => [first_entry, second_entry, None],
        _ => crate::invariant(),
    };
    assert!(match hole {
        0 => source_slots[0].is_none(),
        1 => source_slots[1].is_none(),
        2 => source_slots[2].is_none(),
        _ => false,
    });
    kani::cover!(hole == 0);
    kani::cover!(hole == 1);
    kani::cover!(hole == 2);

    let mut replacement_slots: [Option<ResolverBucketEntry<'static>>; 4] = [None; 4];
    let replacement = Sidecar::from_raw_parts(
        replacement_slots.as_mut_ptr(),
        ResolverBucket::storage_bytes(replacement_slots.len()),
    );
    /* SAFETY: `replacement_slots` is a disjoint, aligned entry array that stays
    live through all assertions; the bucket exclusively initializes and
    publishes its exact four-slot range while the source array is unchanged. */
    unsafe {
        bucket.replace_storage(replacement.cast(), replacement_slots.len());
    }

    assert!(bucket.capacity() == replacement_slots.len());
    assert!(bucket.entry_count() == 2);
    assert!(bucket.get(first_key).is_some());
    assert!(bucket.get(second_key).is_some());
    let first = match bucket.get(first_key) {
        Some(resolver) => resolver,
        None => crate::invariant(),
    };
    let second = match bucket.get(second_key) {
        Some(resolver) => resolver,
        None => crate::invariant(),
    };
    assert!(matches!(first.resolve_decision(), Ok(DecisionArm::Left)));
    assert!(matches!(second.resolve_decision(), Ok(DecisionArm::Right)));
}
