macro_rules! define_public_operation_kernel {
    (
        phases { $($phase:ident),+ $(,)? }
        edges { $($edge:ident => ($expected:ident, $next:ident)),+ $(,)? }
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(in crate::endpoint) enum PublicActiveOp {
            $($phase),+
        }

        impl PublicActiveOp {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            const ALL: &'static [Self] = &[$(Self::$phase),+];
        }

        /// Complete strict lifecycle edges used by the public endpoint operations.
        /// An edge is transient and is never stored in an endpoint or descriptor.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(in crate::endpoint::kernel) enum PublicOpEdge {
            $($edge),+
        }

        impl PublicOpEdge {
            #[cfg(any(kani, all(test, hibana_repo_tests)))]
            const ALL: &'static [Self] = &[$(Self::$edge),+];

            #[inline(always)]
            pub(in crate::endpoint::kernel) const fn expected(self) -> PublicActiveOp {
                match self {
                    $(Self::$edge => PublicActiveOp::$expected),+
                }
            }

            #[inline(always)]
            pub(in crate::endpoint::kernel) const fn next(self) -> PublicActiveOp {
                match self {
                    $(Self::$edge => PublicActiveOp::$next),+
                }
            }
        }
    };
}

pub(super) use define_public_operation_kernel;
