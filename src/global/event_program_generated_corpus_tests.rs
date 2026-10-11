use crate::{g, global::program::Projectable};

use super::ProductionCursorTrace;

type UnitMsg<const L: u8> = g::Msg<L, ()>;
type SendStep<const L: u8> = g::Send<0, 1, UnitMsg<L>>;
type RecvStep<const L: u8> = g::Send<1, 0, UnitMsg<L>>;
type RowStep<const REQ: u8, const ACK: u8> = g::Seq<SendStep<REQ>, RecvStep<ACK>>;

fn send<const L: u8>() -> g::Program<SendStep<L>> {
    g::send::<0, 1, UnitMsg<L>>()
}

fn recv<const L: u8>() -> g::Program<RecvStep<L>> {
    g::send::<1, 0, UnitMsg<L>>()
}

fn row<const REQ: u8, const ACK: u8>() -> g::Program<RowStep<REQ, ACK>> {
    g::seq(send::<REQ>(), recv::<ACK>())
}

fn run_generated_corpus_case(program: &impl Projectable, steps: &[u8]) {
    let mut trace = ProductionCursorTrace::new::<0>(program);
    for &step in steps {
        trace.commit_label(step);
    }
}

#[test]
fn generated_intrinsic_left_spine_route_seq_par_roll() {
    let program = g::route(
        g::seq(
            g::par(row::<101, 102>(), row::<103, 104>()).roll(),
            row::<105, 106>(),
        )
        .roll(),
        row::<107, 108>(),
    )
    .roll();
    run_generated_corpus_case(&program, &[101, 102, 103, 104, 105, 106, 107, 108]);
}

#[test]
fn generated_intrinsic_right_spine_route_seq_par_roll() {
    let program = g::route(
        row::<109, 110>(),
        g::seq(
            row::<111, 112>(),
            g::par(row::<113, 114>(), row::<115, 116>()).roll(),
        )
        .roll(),
    )
    .roll();
    run_generated_corpus_case(&program, &[109, 110, 111, 112, 113, 114, 115, 116]);
}

#[test]
fn generated_intrinsic_deep_left_route_roll_spine() {
    let program = g::route(
        g::route(
            g::route(row::<117, 118>(), row::<119, 120>()).roll(),
            row::<121, 122>(),
        )
        .roll(),
        row::<123, 124>(),
    )
    .roll();
    run_generated_corpus_case(&program, &[117, 118, 119, 120, 121, 122, 123, 124]);
}

#[test]
fn generated_intrinsic_deep_right_route_roll_spine() {
    let program = g::route(
        row::<125, 126>(),
        g::route(
            row::<127, 128>(),
            g::route(row::<129, 130>(), row::<131, 132>()).roll(),
        )
        .roll(),
    )
    .roll();
    run_generated_corpus_case(&program, &[125, 126, 127, 128, 129, 130, 131, 132]);
}

#[test]
fn generated_intrinsic_route_roll_contains_seq_roll_and_par_roll() {
    let program = g::route(
        g::seq(row::<133, 134>(), row::<135, 136>()).roll(),
        g::par(row::<137, 138>(), row::<139, 140>()).roll(),
    )
    .roll();
    run_generated_corpus_case(&program, &[133, 134, 135, 136, 137, 138, 139, 140]);
}

#[test]
fn generated_intrinsic_par_roll_contains_route_roll_and_seq_roll() {
    let program = g::par(
        g::route(row::<141, 142>(), row::<143, 144>()).roll(),
        g::seq(row::<145, 146>(), row::<147, 148>()).roll(),
    )
    .roll();
    run_generated_corpus_case(&program, &[145, 146, 147, 148, 141, 142, 143, 144]);
}

#[test]
fn generated_resolved_left_outer_route_allows_cross_arm_reentry() {
    let program = g::route(
        g::seq(
            g::route(row::<149, 150>(), row::<151, 152>())
                .resolve::<0x0440>()
                .roll(),
            row::<153, 154>(),
        )
        .roll(),
        row::<155, 156>(),
    )
    .resolve::<0x0441>()
    .roll();
    run_generated_corpus_case(&program, &[149, 150, 151, 152, 153, 154, 155, 156]);
}

#[test]
fn generated_resolved_right_outer_route_allows_cross_arm_reentry() {
    let program = g::route(
        row::<157, 158>(),
        g::seq(
            g::route(row::<159, 160>(), row::<161, 162>())
                .resolve::<0x0442>()
                .roll(),
            row::<163, 164>(),
        )
        .roll(),
    )
    .resolve::<0x0443>()
    .roll();
    run_generated_corpus_case(&program, &[157, 158, 159, 160, 161, 162, 163, 164]);
}

#[test]
fn generated_resolved_par_seq_route_roll_mixed_asymmetric() {
    let program = g::route(
        g::par(
            g::seq(
                g::route(row::<165, 166>(), row::<167, 168>())
                    .resolve::<0x0444>()
                    .roll(),
                row::<169, 170>(),
            )
            .roll(),
            row::<171, 172>(),
        )
        .roll(),
        row::<173, 174>(),
    )
    .resolve::<0x0445>()
    .roll();
    run_generated_corpus_case(
        &program,
        &[165, 166, 167, 168, 169, 170, 171, 172, 173, 174],
    );
}

#[test]
fn generated_resolved_mirrored_par_seq_route_roll_mixed_asymmetric() {
    let program = g::route(
        row::<175, 176>(),
        g::par(
            row::<177, 178>(),
            g::seq(
                row::<179, 180>(),
                g::route(row::<181, 182>(), row::<183, 184>())
                    .resolve::<0x0446>()
                    .roll(),
            )
            .roll(),
        )
        .roll(),
    )
    .resolve::<0x0447>()
    .roll();
    run_generated_corpus_case(
        &program,
        &[175, 176, 177, 178, 179, 180, 181, 182, 183, 184],
    );
}
