#![allow(dead_code)]

const BUDGET: hibana::control::LeaseGraphBudget = hibana::control::LeaseGraphBudget::new();
const NEEDS: hibana::control::lease::planner::LeaseFacetNeeds =
    hibana::control::lease::planner::LeaseFacetNeeds::new().with_caps();

const _: () = hibana::control::lease::planner::assert_budget_covers(BUDGET, NEEDS);

fn main() {}
