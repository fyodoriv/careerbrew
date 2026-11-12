#[path = "support/owner_controls.rs"]
mod controls;

#[test]
fn invalid_budget_never_spawns() {
    controls::invalid_budget_never_spawns();
}

#[test]
fn positive_budget_preserves_actual_exit() {
    controls::positive_budget_preserves_actual_exit();
}

#[test]
fn short_budget_kills_and_reaps_actual_child() {
    controls::short_budget_kills_and_reaps_actual_child();
}
