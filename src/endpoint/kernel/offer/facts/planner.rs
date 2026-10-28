use super::super::{
    Arm, OfferScopeProfile, OfferScopeSelection, OperationKernel, ScopeArmMaterializationMeta,
    Transport,
    profile::{
        OfferArmRecvEvidence, OfferControllerCursorArm, OfferControllerLocalEvidence,
        OfferControllerLocalReadiness, OfferCursorReadiness, OfferMaterializationReadiness,
        OfferPassiveEvidence, OfferPassiveReadiness, OfferPassiveReadySignal,
        OfferPassiveRecvEvidence,
    },
};
use super::evidence::OfferIngressEvidence;

struct OfferIngressPlannerInput {
    selection: OfferScopeSelection,
    cursor: OfferCursorReadiness,
}

impl<'r, T> OperationKernel<'_, 'r, T>
where
    T: Transport + 'r,
{
    /// An already committed choice can expose this view's next send/local
    /// action after another co-resident view consumed the branch receive.
    /// This is choice/shape intent only. Final materialization and public
    /// branch commit independently admit the exact event before publication.
    pub(in crate::endpoint::kernel) fn committed_nonreceive_offer_intent(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        index: usize,
    ) -> Option<Arm> {
        let cursor = &self.progress.cursor;
        if !cursor.is_send_at(index) && !cursor.is_local_action_at(index) {
            return None;
        }
        let arm = cursor.route_arm_for_index(scope, index)?;
        let mut committed = true;
        cursor.visit_route_arms_for_index(index, |scope, arm| {
            committed &= self.selected_live_arm_for_scope(scope) == Some(arm);
        });
        if !committed {
            return None;
        }
        Some(Arm::from_raw(arm))
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn committed_nonreceive_offer_arm(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        index: usize,
    ) -> Option<Arm> {
        let arm = self.committed_nonreceive_offer_intent(scope, index)?;
        self.view_event_is_enabled(index, &mut self.preview_queries())
            .then_some(arm)
    }

    pub(super) fn offer_ingress_evidence(
        &mut self,
        selection: OfferScopeSelection,
        profile: OfferScopeProfile,
    ) -> OfferIngressEvidence {
        let input = OfferIngressPlannerInput {
            selection,
            cursor: if self
                .progress
                .cursor
                .is_recv_at(selection.candidate_index.as_usize())
            {
                OfferCursorReadiness::Recv
            } else {
                OfferCursorReadiness::NonRecv
            },
        };

        match profile {
            OfferScopeProfile::ControllerIntrinsic => {
                self.controller_intrinsic_ingress_evidence(input)
            }
            OfferScopeProfile::ControllerDynamic => self.controller_dynamic_ingress_evidence(input),
            OfferScopeProfile::PassiveIntrinsic => self.passive_intrinsic_ingress_evidence(input),
            OfferScopeProfile::PassiveDynamic => self.passive_dynamic_ingress_evidence(input),
        }
    }

    pub(super) fn offer_scope_profile(
        &self,
        scope_id: crate::global::const_dsl::ScopeId,
    ) -> OfferScopeProfile {
        let is_controller = self.is_route_controller(scope_id);
        let is_dynamic = self
            .progress
            .cursor
            .route_scope_resolver(scope_id)
            .is_some();
        match (is_controller, is_dynamic) {
            (true, true) => OfferScopeProfile::ControllerDynamic,
            (true, false) => OfferScopeProfile::ControllerIntrinsic,
            (false, true) => OfferScopeProfile::PassiveDynamic,
            (false, false) => OfferScopeProfile::PassiveIntrinsic,
        }
    }

    fn controller_intrinsic_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::ControllerIntrinsic,
            entry: input.selection.entry_position,
            cursor: input.cursor,
            controller: self.controller_intrinsic_readiness(&input),
            passive: OfferPassiveReadiness::NeedsTransport,
        }
    }

    fn controller_dynamic_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::ControllerDynamic,
            entry: input.selection.entry_position,
            cursor: input.cursor,
            controller: self.controller_dynamic_readiness(&input),
            passive: OfferPassiveReadiness::NeedsTransport,
        }
    }

    fn passive_intrinsic_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::PassiveIntrinsic,
            entry: input.selection.entry_position,
            cursor: input.cursor,
            controller: OfferControllerLocalReadiness::NeedsTransport,
            passive: self.passive_intrinsic_readiness(&input),
        }
    }

    fn passive_dynamic_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::PassiveDynamic,
            entry: input.selection.entry_position,
            cursor: input.cursor,
            controller: OfferControllerLocalReadiness::NeedsTransport,
            passive: self.passive_dynamic_readiness(&input),
        }
    }

    fn controller_intrinsic_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerLocalReadiness {
        let profile = OfferScopeProfile::ControllerIntrinsic;
        profile.controller_local_readiness(self.controller_local_evidence(input))
    }

    fn controller_dynamic_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerLocalReadiness {
        let profile = OfferScopeProfile::ControllerDynamic;
        profile.controller_local_readiness(self.controller_local_evidence(input))
    }

    fn controller_local_evidence(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerLocalEvidence {
        OfferControllerLocalEvidence::new(
            input.cursor,
            self.controller_cursor_arm(input),
            self.controller_materialization_readiness(input),
        )
    }

    fn controller_cursor_arm(&self, input: &OfferIngressPlannerInput) -> OfferControllerCursorArm {
        if self
            .controller_arm_at_index(
                input.selection.scope_id,
                input.selection.entry_index.as_usize(),
            )
            .is_some()
        {
            OfferControllerCursorArm::AtArm
        } else {
            OfferControllerCursorArm::OutsideArm
        }
    }

    fn controller_materialization_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferMaterializationReadiness {
        let Some(arm) = self.selected_live_arm_for_scope(input.selection.scope_id) else {
            return OfferMaterializationReadiness::Ready;
        };
        if self.arm_requires_materialization_ready_evidence_at(
            input.selection.scope_id,
            arm,
            input.selection.entry_index.as_usize(),
        ) && !self.scope_has_ready_arm(input.selection.scope_id, arm)
        {
            OfferMaterializationReadiness::Pending
        } else {
            OfferMaterializationReadiness::Ready
        }
    }

    fn passive_intrinsic_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferPassiveReadiness {
        let materialization = self.compute_scope_arm_materialization_meta(input.selection.scope_id);
        OfferScopeProfile::PassiveIntrinsic.passive_readiness(OfferPassiveEvidence::new(
            self.passive_ready_signal(input),
            self.passive_recv_evidence(input, materialization),
        ))
    }

    fn passive_dynamic_readiness(&self, input: &OfferIngressPlannerInput) -> OfferPassiveReadiness {
        let materialization = self.compute_scope_arm_materialization_meta(input.selection.scope_id);
        OfferScopeProfile::PassiveDynamic.passive_readiness(OfferPassiveEvidence::new(
            self.passive_ready_signal(input),
            self.passive_recv_evidence(input, materialization),
        ))
    }

    fn passive_ready_signal(&self, input: &OfferIngressPlannerInput) -> OfferPassiveReadySignal {
        if self.scope_has_ready_arm_evidence(input.selection.scope_id)
            || self
                .committed_nonreceive_offer_intent(
                    input.selection.scope_id,
                    input.selection.candidate_index.as_usize(),
                )
                .is_some()
        {
            OfferPassiveReadySignal::Known
        } else {
            OfferPassiveReadySignal::Absent
        }
    }

    fn passive_recv_evidence(
        &self,
        input: &OfferIngressPlannerInput,
        materialization: ScopeArmMaterializationMeta,
    ) -> OfferPassiveRecvEvidence {
        if matches!(
            self.arm_recv_evidence(input, 0, materialization),
            OfferArmRecvEvidence::HasRecv
        ) || matches!(
            self.arm_recv_evidence(input, 1, materialization),
            OfferArmRecvEvidence::HasRecv
        ) {
            OfferPassiveRecvEvidence::HasRecv
        } else {
            OfferPassiveRecvEvidence::Recvless
        }
    }

    fn arm_recv_evidence(
        &self,
        input: &OfferIngressPlannerInput,
        arm: u8,
        materialization: ScopeArmMaterializationMeta,
    ) -> OfferArmRecvEvidence {
        let scope_id = input.selection.scope_id;
        if self.arm_has_recv_with_materialization(scope_id, arm, materialization)
            || self
                .compute_passive_arm_recv_meta(scope_id, arm, input.selection.offer_lane)
                .is_recv_step()
        {
            OfferArmRecvEvidence::HasRecv
        } else {
            OfferArmRecvEvidence::Recvless
        }
    }
}
