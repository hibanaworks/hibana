// Shared protocol matrix used by role-program tests and Pico footprint gates.
macro_rules! final_form_protocol {
    (minimal_send_recv) => {
        g::send::<0, 1, Msg<1, ()>>()
    };
    (nested_par_join) => {
        g::seq(
            g::par(
                g::send::<0, 1, Msg<10, ()>>(),
                g::send::<2, 3, Msg<11, ()>>(),
            ),
            g::send::<1, 0, Msg<12, ()>>(),
        )
    };
    (route_with_unselected_nested_par) => {
        g::seq(
            g::route(
                g::send::<0, 1, Msg<20, ()>>(),
                g::seq(
                    g::send::<0, 2, Msg<24, ()>>(),
                    g::par(
                        g::send::<0, 1, Msg<21, ()>>(),
                        g::send::<2, 3, Msg<22, ()>>(),
                    ),
                ),
            )
            .resolve::<0x9120>(),
            g::send::<1, 0, Msg<23, ()>>(),
        )
    };
    (triple_nested_route) => {
        g::seq(
            g::route(
                g::route(
                    g::route(
                        g::send::<0, 1, Msg<30, ()>>(),
                        g::send::<0, 1, Msg<31, ()>>(),
                    ),
                    g::send::<0, 1, Msg<32, ()>>(),
                ),
                g::send::<0, 1, Msg<33, ()>>(),
            ),
            g::send::<1, 0, Msg<34, ()>>(),
        )
    };
    (passive_nested_route_observer) => {{
        let inner = g::route(
            g::send::<0, 1, Msg<40, ()>>(),
            g::send::<0, 1, Msg<41, ()>>(),
        );
        let outer_left = g::seq(inner, g::send::<0, 2, Msg<42, ()>>());
        let outer_right = g::seq(
            g::send::<0, 1, Msg<43, ()>>(),
            g::send::<0, 2, Msg<45, ()>>(),
        );
        g::seq(
            g::route(outer_left, outer_right),
            g::send::<0, 3, Msg<44, ()>>(),
        )
    }};
    (alternating_par_route) => {{
        let inner = g::route(
            g::send::<0, 1, Msg<50, ()>>(),
            g::send::<0, 1, Msg<51, ()>>(),
        );
        let outer_left = g::seq(
            g::par(inner, g::send::<0, 2, Msg<52, ()>>()),
            g::send::<0, 1, Msg<53, ()>>(),
        );
        let outer_right = g::seq(
            g::send::<0, 1, Msg<54, ()>>(),
            g::send::<0, 2, Msg<59, ()>>(),
        );
        let routed = g::route(outer_left, outer_right);
        g::seq(
            g::par(routed, g::send::<0, 3, Msg<55, ()>>()),
            g::send::<0, 3, Msg<56, ()>>(),
        )
    }};
    (huge_legal_choreography) => {
        g::seq(
            g::seq(
                g::seq(
                    g::seq(
                        g::seq(
                            g::seq(
                                g::send::<0, 1, Msg<60, ()>>(),
                                g::send::<1, 0, Msg<61, ()>>(),
                            ),
                            g::seq(
                                g::send::<0, 1, Msg<62, ()>>(),
                                g::send::<1, 0, Msg<63, ()>>(),
                            ),
                        ),
                        g::seq(
                            g::send::<0, 1, Msg<64, ()>>(),
                            g::send::<1, 0, Msg<65, ()>>(),
                        ),
                    ),
                    g::seq(
                        g::route(
                            g::send::<0, 1, Msg<66, ()>>(),
                            g::send::<0, 1, Msg<67, ()>>(),
                        ),
                        g::par(
                            g::send::<0, 1, Msg<68, ()>>(),
                            g::send::<1, 0, Msg<69, ()>>(),
                        ),
                    ),
                ),
                g::seq(
                    g::route(
                        g::seq(
                            g::send::<0, 1, Msg<70, ()>>(),
                            g::send::<1, 0, Msg<71, ()>>(),
                        ),
                        g::seq(
                            g::send::<0, 1, Msg<72, ()>>(),
                            g::send::<1, 0, Msg<73, ()>>(),
                        ),
                    ),
                    g::send::<0, 1, Msg<74, ()>>(),
                ),
            ),
            g::send::<1, 0, Msg<75, ()>>(),
        )
    };
}
