//! Replay the actual compiler's colors against the proved allocation functions.
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

fn actual_colors<Steps: g::ProgramShape + LeanChoreo>(name: &str, source: &mut String) {
    let projection = &g::ProgramProjection::<Steps, 32>::SOURCE_EFF_LIST;
    let colors = (0..projection.len())
        .map(|event| projection.frame_label_at(event).to_string())
        .collect::<std::vec::Vec<_>>()
        .join(", ");
    writeln!(source, "def {name}Choreo : Choreo := {}\n\ndef {name}RustColors : List Nat := [{colors}]\n\n\
        theorem {name}ColorsMatchActualRust :\n  (canonicalProgramSource {name}Choreo).atoms.map ProgramAtomBody.frameLabel = {name}RustColors := by decide\n",
        Steps::lean_source()).unwrap();
}

#[test]
fn actual_rolled_colors_refine_route_paths_and_elastic_owners() {
    const {
        let () = g::ProgramProjection::<Nested, 32>::VALIDATION;
        let () = g::ProgramProjection::<Branch, 32>::VALIDATION;
        let () = g::ProgramProjection::<Parallel, 32>::VALIDATION;
    }
    let nested = &g::ProgramProjection::<Nested, 32>::SOURCE_EFF_LIST;
    assert_eq!(nested.frame_label_at(0), nested.frame_label_at(3));
    assert_eq!(nested.frame_label_at(1), nested.frame_label_at(2));
    assert_ne!(nested.frame_label_at(0), nested.frame_label_at(1));
    let branch = &g::ProgramProjection::<Branch, 32>::SOURCE_EFF_LIST;
    assert_eq!(branch.frame_label_at(0), branch.frame_label_at(1));
    assert_eq!(branch.frame_label_at(2), branch.frame_label_at(3));
    assert_ne!(branch.frame_label_at(0), branch.frame_label_at(2));
    assert_ne!(branch.frame_label_at(0), branch.frame_label_at(4));
    assert_ne!(branch.frame_label_at(2), branch.frame_label_at(4));
    let parallel = &g::ProgramProjection::<Parallel, 32>::SOURCE_EFF_LIST;
    assert_ne!(parallel.atom_at(0).lane, parallel.atom_at(2).lane);
    let mut source = String::from(
        "import Hibana.ElasticFrameDomains\nimport Hibana.RollFrameDomains\nimport Hibana.SourceFrameWord\n\nset_option maxRecDepth 10000\nopen Hibana\n\n",
    );
    actual_colors::<Nested>("nested", &mut source);
    actual_colors::<Branch>("branch", &mut source);
    actual_colors::<Parallel>("parallel", &mut source);
    let branch_program = crate::runtime::program::project::<0>(&g::Program::<Branch>::new());
    writeln!(
        source,
        "def branchWordImage : RustDescriptorImage := {}\n",
        descriptor_source(RoleDescriptorRef::from_resident(
            branch_program.role_image_ref()
        ))
    )
    .unwrap();
    source.push_str(
        r#"
private def conditionalPairKey := (branchWordImage.frameKey? 0).get (by decide)
private def conditionalPairClass : SourceOrderGroup branchWordImage branchChoreo := {
  rows := branchWordImage.frameClassRows conditionalPairKey
  distinct := by decide
  data := (branchWordImage.decodeActorOrder? branchChoreo).get (by decide)
  rolls := branchWordImage.decodeRollRows?.get (by decide)
  choices := (branchWordImage.frameClassChoices? branchChoreo
    conditionalPairKey).get (by decide)
  decoded := (Option.some_get _).symm
  decodedRolls := (Option.some_get _).symm
  nonempty := by decide
  paths := by decide
  choiceMembers := by decide
  slots := by decide
  active := by decide
  ordered := by decide
  uniformReset := by decide
  outsideChoices := by decide
}

theorem actualConditionalTwoLetterClassKeepsBothSourcePaths :
    ∃ group : SourceOrderGroup branchWordImage branchChoreo,
      group.rows = branchWordImage.frameClassRows conditionalPairKey ∧
      group.rows.length = 2 ∧
      group.data.normalized.paths.length = 2 :=
  ⟨conditionalPairClass, rfl, by decide, by decide⟩
"#,
    );
    writeln!(
        source,
        "def bodyChoreo : Choreo := {}\n",
        BranchBody::lean_source()
    )
    .unwrap();
    source.push_str(r#"
def nestedBefore := nestedChoreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody
def nestedAfter := (canonicalProgramSource nestedChoreo).atoms

theorem actualOuterAliasHasItsOriginalColorAndOwner :
    (nestedBefore[0]).frameLabel = (nestedBefore[3]).frameLabel ∧
    elasticFrameOwner (canonicalControlSource nestedChoreo).markers 0 =
      elasticFrameOwner (canonicalControlSource nestedChoreo).markers 3 :=
  a_canonical_rolled_wire_alias_retains_its_actual_source_domain
    (leftSource := nestedBefore[0]) (rightSource := nestedBefore[3])
    (leftOutput := nestedAfter[0]) (rightOutput := nestedAfter[3])
    (choreo := nestedChoreo) (left := 0) (right := 3)
    (by decide) (by decide) (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)

theorem actualInnerAndOuterDomainsRemainDistinct :
    elasticFrameOwner (canonicalControlSource nestedChoreo).markers 0 ≠
      elasticFrameOwner (canonicalControlSource nestedChoreo).markers 1 ∧
    (nestedAfter[0]).frameLabel ≠ (nestedAfter[1]).frameLabel := by decide

def bodyBefore := bodyChoreo.compiledOccurrences.occurrences
def bodyRows := bodyChoreo.globalEventConflicts
def bodyRecolored := recolorRollOccurrences bodyBefore bodyRows

theorem actualOrderedAliasKeepsItsCompleteSourceRoutePath : bodyRows[0] = bodyRows[1] :=
  same_roll_wire_color_retains_its_exact_source_route_path
    (leftSource := bodyBefore[0]) (rightSource := bodyBefore[1])
    (leftOutput := bodyRecolored[0]) (rightOutput := bodyRecolored[1])
    (leftPath := bodyRows[0]) (rightPath := bodyRows[1])
    (left := 0) (right := 1)
    (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (getElem?_pos _ _ (by decide)) (getElem?_pos _ _ (by decide))
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)

theorem actualBranchAndOutsidePathsDoNotAlias :
    (bodyRecolored[0]).frameLabel ≠ (bodyRecolored[2]).frameLabel ∧
    (bodyRecolored[0]).frameLabel ≠ (bodyRecolored[4]).frameLabel ∧
    (bodyRecolored[2]).frameLabel ≠ (bodyRecolored[4]).frameLabel := by decide

theorem exhaustedElasticColorsCannotBecomeAnAcceptedByte :
    firstElasticFrameColor (List.range 256) = 256 := by decide
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("FrameDomainsGenerated.lean"), source).unwrap();
}
