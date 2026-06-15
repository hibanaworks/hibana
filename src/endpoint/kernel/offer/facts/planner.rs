use super::super::{
    CursorEndpoint, OfferScopeProfile, OfferScopeSelection, RouteArmToken,
    ScopeArmMaterializationMeta, Transport,
    profile::{
        OfferArmRecvEvidence, OfferControllerCursorArm, OfferControllerLocalEvidence,
        OfferControllerLocalReadiness, OfferCursorReadiness, OfferEarlyDecisionReadiness,
        OfferEntryPosition, OfferMaterializationReadiness, OfferPassiveAckEvidence,
        OfferPassiveEvidence, OfferPassiveReadiness, OfferPassiveReadySignal,
        OfferPassiveRecvEvidence,
    },
};
use super::evidence::OfferIngressEvidence;

struct OfferIngressPlannerInput {
    selection: OfferScopeSelection,
    entry: OfferEntryPosition,
    materialization: ScopeArmMaterializationMeta,
    preview_route_arm_selection: Option<RouteArmToken>,
    cursor: OfferCursorReadiness,
}

impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    pub(super) fn offer_ingress_evidence(
        &mut self,
        selection: OfferScopeSelection,
        entry: OfferEntryPosition,
        profile: OfferScopeProfile,
        offer_lanes: crate::global::role_program::LaneSetView,
    ) -> OfferIngressEvidence {
        let scope_id = selection.scope_id;
        let input = OfferIngressPlannerInput {
            selection,
            entry,
            materialization: self.selection_materialization_meta(selection),
            preview_route_arm_selection: self
                .preview_scope_ack_token_non_consuming(scope_id, offer_lanes),
            cursor: if self.cursor.is_recv() {
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
        let is_controller = self.cursor.is_route_controller(scope_id);
        let is_dynamic = self
            .cursor
            .route_scope_controller_resolver(scope_id)
            .is_some_and(|(resolver, _, _)| resolver.is_dynamic());
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
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.controller_early_decision_readiness(&input),
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
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.controller_early_decision_readiness(&input),
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
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.passive_early_decision_readiness(&input),
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
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.passive_early_decision_readiness(&input),
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
            .controller_arm_at_cursor(input.selection.scope_id)
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
        let Some(arm) = self.selected_arm_for_scope(input.selection.scope_id) else {
            return OfferMaterializationReadiness::Ready;
        };
        if self.arm_requires_materialization_ready_evidence(input.selection.scope_id, arm)
            && !self.scope_has_ready_arm(input.selection.scope_id, arm)
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
        OfferScopeProfile::PassiveIntrinsic.passive_readiness(OfferPassiveEvidence::new(
            self.passive_ready_signal(input),
            self.passive_recv_evidence(input),
            OfferPassiveAckEvidence::NotMaterializable,
        ))
    }

    fn passive_dynamic_readiness(&self, input: &OfferIngressPlannerInput) -> OfferPassiveReadiness {
        OfferScopeProfile::PassiveDynamic.passive_readiness(OfferPassiveEvidence::new(
            self.passive_ready_signal(input),
            self.passive_recv_evidence(input),
            self.passive_ack_evidence(input),
        ))
    }

    fn passive_ready_signal(&self, input: &OfferIngressPlannerInput) -> OfferPassiveReadySignal {
        if self.scope_has_ready_arm_evidence(input.selection.scope_id)
            || self
                .peek_scope_frame_hint(input.selection.scope_id)
                .is_some()
        {
            OfferPassiveReadySignal::Observed
        } else {
            OfferPassiveReadySignal::Absent
        }
    }

    fn passive_recv_evidence(&self, input: &OfferIngressPlannerInput) -> OfferPassiveRecvEvidence {
        if self.arm_has_recv_with_materialization(
            input.selection.scope_id,
            0,
            input.materialization,
        ) || self.arm_has_recv_with_materialization(
            input.selection.scope_id,
            1,
            input.materialization,
        ) {
            OfferPassiveRecvEvidence::HasRecv
        } else {
            OfferPassiveRecvEvidence::Recvless
        }
    }

    fn passive_ack_evidence(&self, input: &OfferIngressPlannerInput) -> OfferPassiveAckEvidence {
        let Some(token) = input.preview_route_arm_selection else {
            return OfferPassiveAckEvidence::NotMaterializable;
        };
        let arm = token.arm().as_u8();
        if self.scope_has_ready_arm(input.selection.scope_id, arm) {
            return OfferPassiveAckEvidence::Materializable;
        }
        match self.arm_recv_evidence(input.selection.scope_id, arm, input.materialization) {
            OfferArmRecvEvidence::Recvless => OfferPassiveAckEvidence::Materializable,
            OfferArmRecvEvidence::HasRecv => OfferPassiveAckEvidence::NotMaterializable,
        }
    }

    fn controller_early_decision_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferEarlyDecisionReadiness {
        self.arm_decision_readiness(input, input.preview_route_arm_selection)
    }

    fn passive_early_decision_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferEarlyDecisionReadiness {
        let Some(token) = input.preview_route_arm_selection else {
            return self.arm_decision_readiness(input, None);
        };
        let decision = match self.arm_recv_evidence(
            input.selection.scope_id,
            token.arm().as_u8(),
            input.materialization,
        ) {
            OfferArmRecvEvidence::Recvless => Some(token),
            OfferArmRecvEvidence::HasRecv => None,
        };
        self.arm_decision_readiness(input, decision)
    }

    fn arm_decision_readiness(
        &self,
        input: &OfferIngressPlannerInput,
        decision: Option<RouteArmToken>,
    ) -> OfferEarlyDecisionReadiness {
        OfferEarlyDecisionReadiness::from_arm_evidence(decision.map(|token| {
            self.arm_recv_evidence(
                input.selection.scope_id,
                token.arm().as_u8(),
                input.materialization,
            )
        }))
    }

    fn arm_recv_evidence(
        &self,
        scope_id: crate::global::const_dsl::ScopeId,
        arm: u8,
        materialization: ScopeArmMaterializationMeta,
    ) -> OfferArmRecvEvidence {
        if self.arm_has_recv_with_materialization(scope_id, arm, materialization) {
            OfferArmRecvEvidence::HasRecv
        } else {
            OfferArmRecvEvidence::Recvless
        }
    }
}
