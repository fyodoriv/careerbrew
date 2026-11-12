#[path = "support/http_controls.rs"]
mod controls;
use controls::capture_actions;

#[test]
fn captures_actual_twenty_one_case_handler_corpus() {
    controls::captures_actual_twenty_one_case_handler_corpus();
}

#[test]
fn rejects_real_catalog_removal_in_both_servers() {
    controls::rejects_real_catalog_removal_in_both_servers();
}

#[test]
fn rejects_real_weakened_header() {
    controls::rejects_real_weakened_header();
}

#[test]
fn rejects_real_authentication_bypass() {
    controls::rejects_real_authentication_bypass();
}

#[test]
fn rejects_real_wrong_loader_root() {
    controls::rejects_real_wrong_loader_root();
}

#[test]
fn rejects_real_served_source_change() {
    controls::rejects_real_served_source_change();
}

#[test]
fn rejects_real_stdout_canary() {
    controls::rejects_real_stdout_canary();
}

#[test]
fn rejects_real_stderr_canary() {
    controls::rejects_real_stderr_canary();
}

#[test]
fn rejects_real_retained_canary() {
    controls::rejects_real_retained_canary();
}

#[test]
fn rejects_real_extra_write() {
    controls::rejects_real_extra_write();
}

#[test]
fn retains_real_skipped_and_nonzero_exit_results() {
    controls::retains_real_skipped_and_nonzero_exit_results();
}

#[test]
fn rejects_real_wire_defects_and_reaps_hanging_handler() {
    controls::rejects_real_wire_defects_and_reaps_hanging_handler();
}

#[test]
fn preserves_shared_cli_capture_contract() {
    controls::preserves_shared_cli_capture_contract();
}

#[test]
fn publishes_complete_readiness_after_paused_real_write() {
    controls::publishes_complete_readiness_after_paused_real_write();
}

#[test]
fn rejects_fully_published_malformed_readiness() {
    controls::rejects_fully_published_malformed_readiness();
}

#[test]
fn rejects_changed_reference_inputs_before_python() {
    controls::rejects_changed_reference_inputs_before_python();
}
