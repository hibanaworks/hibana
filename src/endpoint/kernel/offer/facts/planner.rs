use super::super::{
    Clock, CursorEndpoint, EpochTable, LabelUniverse, MintConfigMarker, OfferScopeProfile,
    OfferScopeSelection, RouteDecisionToken, ScopeArmMaterializationMeta, Transport,
    profile::{
        OfferArmRecvEvidence, OfferAuthorityRole, OfferControllerCursorArm,
        OfferControllerReadiness, OfferControllerSkipEvidence, OfferControllerSkipReadiness,
        OfferCursorReadiness, OfferEarlyDecisionReadiness, OfferEntryPosition,
        OfferMaterializationReadiness, OfferPassiveAckEvidence, OfferPassiveEvidence,
        OfferPassiveReadiness, OfferPassiveReadySignal, OfferPassiveRecvEvidence,
        OfferRouteScopeKind,
    },
};
use super::evidence::OfferIngressEvidence;

struct OfferIngressPlannerInput {
    selection: OfferScopeSelection,
    entry: OfferEntryPosition,
    materialization: ScopeArmMaterializationMeta,
    preview_route_decision: Option<RouteDecisionToken>,
    cursor: OfferCursorReadiness,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
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
            preview_route_decision: self
                .preview_scope_ack_token_non_consuming(scope_id, offer_lanes),
            cursor: if self.cursor.is_recv() {
                OfferCursorReadiness::Recv
            } else {
                OfferCursorReadiness::NonRecv
            },
        };

        match profile {
            OfferScopeProfile::ControllerStatic => self.controller_static_ingress_evidence(input),
            OfferScopeProfile::ControllerDynamic => self.controller_dynamic_ingress_evidence(input),
            OfferScopeProfile::PassiveStatic => self.passive_static_ingress_evidence(input),
            OfferScopeProfile::PassiveDynamic => self.passive_dynamic_ingress_evidence(input),
        }
    }

    pub(super) fn offer_scope_profile(
        &self,
        scope_id: crate::global::const_dsl::ScopeId,
    ) -> OfferScopeProfile {
        let role = if self.cursor.is_route_controller(scope_id) {
            OfferAuthorityRole::Controller
        } else {
            OfferAuthorityRole::Passive
        };
        let kind = match self.cursor.route_scope_controller_policy(scope_id) {
            Some((policy, _, _, _)) if policy.is_dynamic() => OfferRouteScopeKind::Dynamic,
            _ => OfferRouteScopeKind::Static,
        };
        OfferScopeProfile::from_evidence(role, kind)
    }

    fn controller_static_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::ControllerStatic,
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.controller_early_decision_readiness(&input),
            controller: self.controller_static_readiness(&input),
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

    fn passive_static_ingress_evidence(
        &self,
        input: OfferIngressPlannerInput,
    ) -> OfferIngressEvidence {
        OfferIngressEvidence {
            profile: OfferScopeProfile::PassiveStatic,
            entry: input.entry,
            cursor: input.cursor,
            early_decision: self.passive_early_decision_readiness(&input),
            controller: OfferControllerReadiness {
                skip: OfferControllerSkipReadiness::NeedsTransport,
            },
            passive: self.passive_static_readiness(&input),
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
            controller: OfferControllerReadiness {
                skip: OfferControllerSkipReadiness::NeedsTransport,
            },
            passive: self.passive_dynamic_readiness(&input),
        }
    }

    fn controller_static_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerReadiness {
        let profile = OfferScopeProfile::ControllerStatic;
        OfferControllerReadiness {
            skip: profile.controller_skip_readiness(self.controller_skip_evidence(input)),
        }
    }

    fn controller_dynamic_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerReadiness {
        let profile = OfferScopeProfile::ControllerDynamic;
        OfferControllerReadiness {
            skip: profile.controller_skip_readiness(self.controller_skip_evidence(input)),
        }
    }

    fn controller_skip_evidence(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferControllerSkipEvidence {
        OfferControllerSkipEvidence::new(
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
            OfferControllerCursorArm::Present
        } else {
            OfferControllerCursorArm::Missing
        }
    }

    fn controller_materialization_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferMaterializationReadiness {
        match self.selected_arm_for_scope(input.selection.scope_id) {
            Some(arm)
                if self
                    .arm_requires_materialization_ready_evidence(input.selection.scope_id, arm)
                    && !self.scope_has_ready_arm(input.selection.scope_id, arm) =>
            {
                OfferMaterializationReadiness::Pending
            }
            _ => OfferMaterializationReadiness::Ready,
        }
    }

    fn passive_static_readiness(&self, input: &OfferIngressPlannerInput) -> OfferPassiveReadiness {
        OfferScopeProfile::PassiveStatic.passive_readiness(OfferPassiveEvidence::new(
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
            OfferPassiveReadySignal::Present
        } else {
            OfferPassiveReadySignal::Missing
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
        let Some(token) = input.preview_route_decision else {
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
        self.arm_decision_readiness(input, input.preview_route_decision)
    }

    fn passive_early_decision_readiness(
        &self,
        input: &OfferIngressPlannerInput,
    ) -> OfferEarlyDecisionReadiness {
        let decision = match input.preview_route_decision {
            Some(token)
                if self
                    .arm_recv_evidence(
                        input.selection.scope_id,
                        token.arm().as_u8(),
                        input.materialization,
                    )
                    .is_recvless() =>
            {
                Some(token)
            }
            _ => None,
        };
        self.arm_decision_readiness(input, decision)
    }

    fn arm_decision_readiness(
        &self,
        input: &OfferIngressPlannerInput,
        decision: Option<RouteDecisionToken>,
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
