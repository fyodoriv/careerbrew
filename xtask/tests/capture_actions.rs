#[path = "support/actions_controls.rs"]
mod controls;

#[test]
fn captures_real_reference_schema_and_module_provenance() {
    controls::captures_real_reference_schema_and_module_provenance();
}

#[test]
fn rejects_same_removed_action_in_both_real_children() {
    controls::rejects_same_removed_action_in_both_real_children();
}

#[test]
fn rejects_same_argument_and_delivery_changes_in_real_children() {
    controls::rejects_same_argument_and_delivery_changes_in_real_children();
}

#[test]
fn rejects_malformed_action_and_argument_types_in_real_children() {
    controls::rejects_malformed_action_and_argument_types_in_real_children();
}

#[test]
fn rejects_actual_stdout_canary() {
    controls::rejects_actual_stdout_canary();
}

#[test]
fn rejects_actual_stderr_canary() {
    controls::rejects_actual_stderr_canary();
}

#[test]
fn scans_actual_retained_canary_content_before_hashing() {
    controls::scans_actual_retained_canary_content_before_hashing();
}

#[test]
fn rejects_actual_extra_retained_file() {
    controls::rejects_actual_extra_retained_file();
}

#[test]
fn rejects_actual_skipped_cli_entry() {
    controls::rejects_actual_skipped_cli_entry();
}

#[test]
fn rejects_observed_nonzero_exit() {
    controls::rejects_observed_nonzero_exit();
}

#[test]
fn cleans_up_owned_descendant_process_group() {
    controls::cleans_up_owned_descendant_process_group();
}

#[test]
fn rejects_wrong_loader_root_and_modified_input_evidence() {
    controls::rejects_wrong_loader_root_and_modified_input_evidence();
}

#[test]
fn retains_leader_ownership_until_group_cleanup() {
    controls::retains_leader_ownership_until_group_cleanup();
}

#[test]
fn distinguishes_owned_zombie_group_from_live_descendant() {
    controls::distinguishes_owned_zombie_group_from_live_descendant();
}
