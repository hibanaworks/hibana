//! Check the wire identity from the same public programs used for execution.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use std::{
    fmt::Write,
    string::{String, ToString},
};

type A = g::Msg<61, u32>;
type B = g::Msg<62, u32>;
type C = g::Msg<63, u32>;
type D = g::Msg<64, u32>;
type E = g::Msg<65, u32>;
type Nested = g::Roll<
    g::Seq<
        g::Send<0, 1, A>,
        g::Seq<g::Roll<g::Seq<g::Send<0, 1, B>, g::Send<0, 1, C>>>, g::Send<0, 1, D>>,
    >,
>;
type BranchBody = g::Seq<
    g::Route<
        g::Seq<g::Send<0, 1, A>, g::Send<0, 1, B>>,
        g::Seq<g::Send<0, 1, C>, g::Send<0, 1, D>>,
    >,
    g::Send<0, 1, E>,
>;
type Branch = g::Roll<BranchBody>;
type Parallel = g::Par<
    g::Roll<g::Seq<g::Send<0, 1, A>, g::Send<0, 1, B>>>,
    g::Roll<g::Seq<g::Send<0, 1, C>, g::Send<0, 1, D>>>,
>;

type M<const L: u8> = g::Msg<L, ()>;
type PipelinedRoute = g::Seq<
    g::Route<
        g::Seq<g::Send<0, 1, M<0>>, g::Seq<g::Send<0, 2, M<1>>, g::Send<1, 2, M<2>>>>,
        g::Seq<
            g::Send<0, 1, M<3>>,
            g::Seq<g::Send<1, 2, M<4>>, g::Seq<g::Send<2, 1, M<5>>, g::Send<1, 0, M<6>>>>,
        >,
    >,
    g::Seq<
        g::Send<0, 1, M<7>>,
        g::Seq<g::Send<1, 2, M<8>>, g::Seq<g::Send<2, 1, M<9>>, g::Send<1, 0, M<10>>>>,
    >,
>;

fn actual_sites<const ROLE: u8>(
    (name, choreography): (&str, String),
    program: &crate::runtime::program::RoleProgram<ROLE>,
    source: &mut String,
) {
    let image = program.role_image_ref();
    let labels = (0..image.local_step_count())
        .map(|index| {
            let node = image.local_step_node(index).unwrap();
            let (event, label) = match node.action() {
                LocalAction::Send {
                    eff_index,
                    frame_label,
                    ..
                }
                | LocalAction::Recv {
                    eff_index,
                    frame_label,
                    ..
                }
                | LocalAction::Local {
                    eff_index,
                    frame_label,
                    ..
                } => (eff_index, frame_label),
                LocalAction::Terminate => panic!("event row cannot terminate"),
            };
            assert_eq!(usize::from(label), event.dense_ordinal());
            label.to_string()
        })
        .collect::<std::vec::Vec<_>>()
        .join(", ");
    let descriptor = descriptor_source(RoleDescriptorRef::from_resident(image));
    writeln!(source, "def {name}Choreo : Choreo := {choreography}\n\
        def {name}Image : RustDescriptorImage := {descriptor}\n\
        def {name}RustSites : List Nat := [{labels}]\n\
        theorem {name}SitesMatchTheExecutedImage :\n  {name}Image.decodeEventFrameLabels? = some {name}RustSites := by decide\n\
        theorem {name}SitesMatchTheGlobalProtocol :\n  {name}RustSites = {name}Choreo.canonicalRoleEffIndices {ROLE} := by decide\n\
        theorem {name}DistinctSourceSitesHaveDistinctWireIdentities :\n  {name}RustSites.Pairwise (· ≠ ·) := by decide\n").unwrap();
}

#[test]
fn actual_rolled_frame_identity_keeps_each_source_site_distinct() {
    let nested = crate::runtime::program::project::<0>(&g::Program::<Nested>::new());
    let branch = crate::runtime::program::project::<0>(&g::Program::<Branch>::new());
    let parallel = crate::runtime::program::project::<0>(&g::Program::<Parallel>::new());
    let mut source = String::from("import Hibana.SourceFrameWord\n\nopen Hibana\n\n");
    actual_sites(("nested", Nested::lean_source()), &nested, &mut source);
    actual_sites(("branch", Branch::lean_source()), &branch, &mut source);
    actual_sites(
        ("parallel", Parallel::lean_source()),
        &parallel,
        &mut source,
    );
    let global = g::Program::<PipelinedRoute>::new();
    let owner = crate::runtime::program::project::<0>(&global);
    let worker = crate::runtime::program::project::<1>(&global);
    let service = crate::runtime::program::project::<2>(&global);
    actual_sites(
        ("pipelinedOwner", PipelinedRoute::lean_source()),
        &owner,
        &mut source,
    );
    actual_sites(
        ("pipelinedWorker", PipelinedRoute::lean_source()),
        &worker,
        &mut source,
    );
    actual_sites(
        ("pipelinedService", PipelinedRoute::lean_source()),
        &service,
        &mut source,
    );
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("FrameDomainsGenerated.lean"), source).unwrap();
}
