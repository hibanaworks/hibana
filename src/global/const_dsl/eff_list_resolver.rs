use super::{EffList, ScopeKind};

impl EffList {
    pub(crate) const fn dynamic_resolver_source_status(&self) -> u8 {
        let mut offset = 0usize;
        while offset < self.len() {
            if let Some((resolver, scope)) = self.resolver_with_scope(offset)
                && resolver.is_dynamic()
                && !matches!(scope.kind(), ScopeKind::Route)
            {
                return 1;
            }
            offset += 1;
        }
        0
    }
}
