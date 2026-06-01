use super::{EffList, ScopeKind};

impl EffList {
    pub(crate) const fn dynamic_policy_source_status(&self) -> u8 {
        let mut offset = 0usize;
        while offset < self.len() {
            if let Some((policy, scope)) = self.policy_with_scope(offset)
                && policy.is_dynamic()
            {
                if !matches!(scope.kind(), ScopeKind::Route) {
                    return 1;
                }
                let Some(control) = self.control_spec_at(offset) else {
                    return 2;
                };
                if !matches!(
                    control.op(),
                    crate::control::cap::mint::ControlOp::RouteDecision
                        | crate::control::cap::mint::ControlOp::LoopContinue
                        | crate::control::cap::mint::ControlOp::LoopBreak
                ) {
                    return 3;
                }
            }
            offset += 1;
        }
        0
    }
}
