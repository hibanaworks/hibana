use super::dependency_conflict_for_scope;
use crate::global::{
    const_dsl::{EffList, ScopeId, ScopeKind},
    typestate::LocalConflict,
};

const SCOPE_KIND_COUNT: usize = 3;
const SCOPE_ORDINAL_COUNT: usize = ScopeId::LOCAL_CAPACITY as usize;

/// Immutable compiler facts shared by every role projection. The index is
/// exactly the encoded scope kind/ordinal domain, including sparse ordinals;
/// no event-count ceiling or runtime storage is introduced.
pub(crate) struct ScopeFacts {
    parents: [[Option<LocalConflict>; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
    route_count: usize,
}

impl ScopeFacts {
    pub(crate) const fn new<const E: usize>(source: &EffList<E>) -> Self {
        let markers = source.scope_markers();
        let mut facts = Self {
            parents: [[None; SCOPE_ORDINAL_COUNT]; SCOPE_KIND_COUNT],
            route_count: 0,
        };
        let mut index = 0usize;
        while index < markers.len() {
            let marker = markers.at(index);
            if marker.event.is_primary_enter() {
                let scope = marker.scope_id;
                let kind = Self::kind_index(scope);
                let ordinal = scope.local_ordinal() as usize;
                if facts.parents[kind][ordinal].is_some() {
                    panic!("duplicate primary scope identity");
                }
                facts.parents[kind][ordinal] =
                    Some(dependency_conflict_for_scope(markers, source.len(), scope));
                if matches!(scope.kind(), Some(ScopeKind::Route)) {
                    facts.route_count += 1;
                }
            }
            index += 1;
        }
        facts
    }

    #[inline(always)]
    const fn kind_index(scope: ScopeId) -> usize {
        match scope.kind() {
            Some(ScopeKind::Route) => 0,
            Some(ScopeKind::Roll) => 1,
            Some(ScopeKind::Parallel) => 2,
            None => crate::invariant(),
        }
    }

    #[inline(always)]
    pub(in crate::global::role_program::image_impl) const fn conflict(
        &self,
        scope: ScopeId,
    ) -> LocalConflict {
        match self.parents[Self::kind_index(scope)][scope.local_ordinal() as usize] {
            Some(conflict) => conflict,
            None => panic!("scope projection fact missing"),
        }
    }

    #[inline(always)]
    pub(super) const fn route_count(&self) -> usize {
        self.route_count
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{
        eff::{EffAtom, EventOrigin},
        g::{Msg, Par, ProgramSourceData, Roll, Route, Send},
    };

    #[test]
    fn cached_parent_facts_match_the_source_definition_for_every_scope() {
        type Body = Roll<
            Par<
                Route<Send<0, 1, Msg<1, ()>>, Send<0, 1, Msg<2, ()>>>,
                Route<
                    Send<0, 2, Msg<3, ()>>,
                    Roll<Route<Send<0, 2, Msg<4, ()>>, Send<0, 2, Msg<5, ()>>>>,
                >,
            >,
        >;
        let source = ProgramSourceData::<32>::lower::<Body>();
        let source = source.eff_list();
        let facts = ScopeFacts::new(source);
        let markers = source.scope_markers();
        let mut routes = 0;
        for index in 0..markers.len() {
            let marker = markers.at(index);
            if marker.event.is_primary_enter() {
                assert_eq!(
                    facts.conflict(marker.scope_id),
                    dependency_conflict_for_scope(markers, source.len(), marker.scope_id)
                );
                if matches!(marker.scope_id.kind(), Some(ScopeKind::Route)) {
                    routes += 1;
                }
            }
        }
        assert_eq!(facts.route_count(), routes);
    }

    #[test]
    fn sparse_last_encoded_scope_ordinal_has_an_exact_cache_slot() {
        let mut source = EffList::<8>::new_partitioned(2, 4, 1);
        let atom = EffAtom {
            from: 0,
            to: 1,
            label: 1,
            payload_schema: 1,
            origin: EventOrigin::User,
            lane: 0,
        };
        source.push_event_mut(atom);
        source.push_event_mut(EffAtom { label: 2, ..atom });
        let scope = ScopeId::route(ScopeId::LOCAL_CAPACITY - 1);
        source.push_route_scope_mut(
            scope,
            0,
            1,
            2,
            crate::global::const_dsl::ReentryMark::SinglePass,
        );
        let facts = ScopeFacts::new(&source);
        assert_eq!(facts.conflict(scope), LocalConflict::Unconditional);
        assert_eq!(facts.route_count(), 1);
    }

    #[test]
    #[should_panic(expected = "scope projection fact missing")]
    fn absent_identity_is_rejected_instead_of_becoming_a_root() {
        ScopeFacts::new(&EffList::<1>::new()).conflict(ScopeId::route(0));
    }
}
