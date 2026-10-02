use super::*;

#[test]
#[ignore = "host-only parallel descriptor proof artifact export"]
fn export_parallel_dependencies_for_lean() {
    fn source<Steps: LeanChoreo>(_: &g::Program<Steps>) -> String {
        Steps::lean_source()
    }
    let tree = g::seq(
        g::par(
            g::par(
                g::par(
                    g::send::<0, 1, g::Msg<1, ()>>(),
                    g::send::<0, 2, g::Msg<2, ()>>(),
                ),
                g::par(
                    g::send::<0, 1, g::Msg<3, ()>>(),
                    g::send::<0, 2, g::Msg<4, ()>>(),
                ),
            ),
            g::par(
                g::par(
                    g::send::<0, 1, g::Msg<5, ()>>(),
                    g::send::<0, 2, g::Msg<6, ()>>(),
                ),
                g::par(
                    g::send::<0, 1, g::Msg<7, ()>>(),
                    g::send::<0, 2, g::Msg<8, ()>>(),
                ),
            ),
        ),
        g::send::<0, 1, g::Msg<9, ()>>(),
    );
    let abi = g::route(
        g::seq(
            g::send::<1, 0, g::Msg<5, ()>>(),
            g::send::<0, 1, g::Msg<6, ()>>(),
        ),
        g::seq(
            g::send::<1, 0, g::Msg<7, ()>>(),
            g::send::<0, 1, g::Msg<8, ()>>(),
        ),
    )
    .roll();
    let admission = g::seq(
        g::send::<0, 2, g::Msg<9, ()>>(),
        g::route(
            g::send::<2, 0, g::Msg<10, ()>>(),
            g::send::<2, 0, g::Msg<11, ()>>(),
        ),
    )
    .roll();
    let independent = g::par(
        g::par(
            abi,
            g::par(
                g::send::<0, 2, g::Msg<12, ()>>(),
                g::send::<0, 2, g::Msg<13, ()>>(),
            ),
        ),
        admission,
    );
    let prefix = g::seq(
        g::send::<0, 1, g::Msg<10, u32>>(),
        g::par(
            g::send::<0, 1, g::Msg<11, u32>>(),
            g::send::<0, 1, g::Msg<20, u32>>(),
        ),
    );
    let nested_prefix = g::seq(
        g::send::<0, 1, g::Msg<10, u32>>(),
        g::par(
            g::seq(
                g::send::<0, 1, g::Msg<12, u32>>(),
                g::par(
                    g::send::<0, 1, g::Msg<13, u32>>(),
                    g::send::<0, 1, g::Msg<14, u32>>(),
                ),
            ),
            g::par(
                g::send::<0, 1, g::Msg<15, u32>>(),
                g::send::<0, 1, g::Msg<16, u32>>(),
            ),
        ),
    );
    let mut generated = format!(
        "import Hibana\n\nset_option maxRecDepth 10000\n\ndef parallelTree : Hibana.Choreo := {}\n\ndef parallelIndependent : Hibana.Choreo := {}\n\n",
        source(&tree),
        source(&independent),
    );
    generated.push_str(&format!("def parallelPrefix : Hibana.Choreo := {}\n\ndef parallelNestedPrefix : Hibana.Choreo := {}\n\n", source(&prefix), source(&nested_prefix)));
    for certificate in [
        projection_certificate_source::<0>(&prefix, "parallelPrefix", "parallelPrefixRole0"),
        projection_certificate_source::<1>(&prefix, "parallelPrefix", "parallelPrefixRole1"),
        projection_certificate_source::<0>(
            &nested_prefix,
            "parallelNestedPrefix",
            "parallelNestedPrefixRole0",
        ),
        projection_certificate_source::<1>(
            &nested_prefix,
            "parallelNestedPrefix",
            "parallelNestedPrefixRole1",
        ),
        projection_certificate_source::<0>(&tree, "parallelTree", "parallelTreeRole0"),
        projection_certificate_source::<1>(&tree, "parallelTree", "parallelTreeRole1"),
        projection_certificate_source::<2>(&tree, "parallelTree", "parallelTreeRole2"),
        projection_certificate_source::<0>(
            &independent,
            "parallelIndependent",
            "parallelIndependentRole0",
        ),
        projection_certificate_source::<1>(
            &independent,
            "parallelIndependent",
            "parallelIndependentRole1",
        ),
        projection_certificate_source::<2>(
            &independent,
            "parallelIndependent",
            "parallelIndependentRole2",
        ),
    ] {
        generated.push_str(&certificate);
        generated.push('\n');
    }
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof/ParallelGenerated.lean");
    fs::create_dir_all(output.parent().unwrap()).expect("create parallel proof directory");
    fs::write(&output, generated).expect("write parallel descriptor certificates");
    println!("parallel-proof-artifact path={}", output.display());
}
