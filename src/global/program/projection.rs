pub(crate) mod seal {
    pub trait Sealed: Copy {
        type Steps: crate::g::ProgramShape;
        fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE>;
    }
}

/// A sealed, zero-sized Hibana choreography that can be composed and projected.
///
/// Return `impl Projectable` to let Rust infer the choreography's step-list type.
/// The same value can be passed to `g::seq`, `g::route`, `g::par`, `g::diagnose`, or `project`.
/// Composition retains the underlying step-list and the existing projection
/// checks; it does not allocate, erase messages, or introduce a runtime layer.
///
/// ```
/// use hibana::{g, runtime::program::{Projectable, project}};
/// fn request() -> impl Projectable { g::send::<0, 1, g::Msg<1, u32>>() }
/// fn reply() -> impl Projectable { g::send::<1, 0, g::Msg<2, u32>>() }
/// fn exchange() -> impl Projectable { g::seq(request(), reply()) }
/// let conversation = g::seq(exchange(), exchange());
/// let client = project::<0, _>(&conversation);
/// let server = project::<1, _>(&conversation);
/// ```
///
/// Only Hibana's constructors can establish this contract. An external type
/// cannot supply its own projection implementation.
///
/// ```compile_fail
/// use hibana::runtime::program::Projectable;
/// #[derive(Clone, Copy)]
/// struct Forged;
/// impl Projectable for Forged {}
/// ```
#[diagnostic::on_unimplemented(
    message = "value is not a projectable hibana choreography",
    label = "expected a hibana choreography built with `hibana::g`"
)]
pub trait Projectable: seal::Sealed {
    /// Mark an opaque choreography fragment as a reentry scope.
    fn roll(self) -> crate::g::Program<crate::g::Roll<Self::Steps>> {
        crate::g::Program::new()
    }
}

impl<P> Projectable for P where P: seal::Sealed {}

#[cfg(test)]
mod tests {
    use super::Projectable;
    use crate::{g, runtime::program::project};

    fn request() -> impl Projectable {
        g::send::<0, 1, g::Msg<1, u32>>()
    }
    fn reply() -> impl Projectable {
        g::send::<1, 0, g::Msg<2, u32>>()
    }
    fn exchange() -> impl Projectable {
        g::seq(request(), reply())
    }

    fn same_images<A: Projectable, B: Projectable>(a: A, b: B) {
        assert_eq!(core::mem::size_of_val(&a), 0);
        assert_eq!(core::mem::size_of_val(&b), 0);
        assert!(core::ptr::eq(
            project::<0, _>(&a).role_image_ref(),
            project::<0, _>(&b).role_image_ref()
        ));
        assert!(core::ptr::eq(
            project::<1, _>(&a).role_image_ref(),
            project::<1, _>(&b).role_image_ref()
        ));
    }

    #[test]
    fn opaque_sequence_keeps_the_exact_projection() {
        same_images(
            g::seq(exchange(), exchange()),
            g::seq(
                g::seq(
                    g::send::<0, 1, g::Msg<1, u32>>(),
                    g::send::<1, 0, g::Msg<2, u32>>(),
                ),
                g::seq(
                    g::send::<0, 1, g::Msg<1, u32>>(),
                    g::send::<1, 0, g::Msg<2, u32>>(),
                ),
            ),
        );
    }

    #[test]
    fn opaque_route_and_roll_keep_the_exact_projection() {
        fn other() -> impl Projectable {
            g::send::<0, 1, g::Msg<3, ()>>()
        }
        fn selection() -> impl Projectable {
            g::route(exchange(), other())
        }
        same_images(
            selection().roll(),
            g::route(
                g::seq(
                    g::send::<0, 1, g::Msg<1, u32>>(),
                    g::send::<1, 0, g::Msg<2, u32>>(),
                ),
                g::send::<0, 1, g::Msg<3, ()>>(),
            )
            .roll(),
        );
    }

    #[test]
    fn opaque_parallel_keeps_the_exact_projection() {
        fn independent() -> impl Projectable {
            g::send::<2, 3, g::Msg<4, ()>>()
        }
        let opaque = g::par(exchange(), independent());
        let explicit = g::par(
            g::seq(
                g::send::<0, 1, g::Msg<1, u32>>(),
                g::send::<1, 0, g::Msg<2, u32>>(),
            ),
            g::send::<2, 3, g::Msg<4, ()>>(),
        );
        same_images(opaque, explicit);
        assert!(core::ptr::eq(
            project::<2, _>(&opaque).role_image_ref(),
            project::<2, _>(&explicit).role_image_ref()
        ));
        assert!(core::ptr::eq(
            project::<3, _>(&opaque).role_image_ref(),
            project::<3, _>(&explicit).role_image_ref()
        ));
    }

    #[test]
    fn opaque_resolved_route_keeps_the_exact_projection() {
        same_images(
            g::route(request(), request()).resolve::<17>(),
            g::route(
                g::send::<0, 1, g::Msg<1, u32>>(),
                g::send::<0, 1, g::Msg<1, u32>>(),
            )
            .resolve::<17>(),
        );
    }

    #[test]
    fn opaque_diagnostics_preserve_acceptance_and_rejection() {
        assert_eq!(g::diagnose(&exchange()), None);
        fn invalid() -> impl Projectable {
            g::route(request(), reply())
        }
        let expected = g::diagnose(&g::route(
            g::send::<0, 1, g::Msg<1, u32>>(),
            g::send::<1, 0, g::Msg<2, u32>>(),
        ));
        assert!(expected.is_some());
        assert_eq!(g::diagnose(&invalid()), expected);
    }
}
