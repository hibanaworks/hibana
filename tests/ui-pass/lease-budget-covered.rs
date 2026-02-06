#![allow(dead_code)]

use hibana::control::ResourceKind;

const BUDGET: hibana::control::LeaseGraphBudget =
    hibana::control::LeaseGraphBudget::new().include_atom(
        hibana::runtime::consts::LABEL_MGMT_LOAD_BEGIN,
        Some(hibana::control::cap::resource_kinds::LoadBeginKind::TAG),
        hibana::g::const_dsl::HandlePlan::none(),
    );

const NEEDS: hibana::control::lease::planner::LeaseFacetNeeds =
    hibana::control::lease::planner::LeaseFacetNeeds::new().with_slots();

const _: () = hibana::control::lease::planner::assert_budget_covers(BUDGET, NEEDS);

fn main() {}
