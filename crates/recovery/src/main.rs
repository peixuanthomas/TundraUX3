use recovery::{RESTART_EXIT_CODE, RecoveryHandoffV1, read_handoff, run, write_restart_request};
use std::path::PathBuf;

fn main() {
    let argument_path = std::env::args_os().skip(1).find_map(|argument| {
        argument
            .to_string_lossy()
            .strip_prefix("--handoff=")
            .map(PathBuf::from)
    });
    let environment_path = std::env::var_os("TUNDRA_RECOVERY_HANDOFF_PATH").map(PathBuf::from);
    let inline = std::env::var("TUNDRA_RECOVERY_HANDOFF_JSON").ok();
    let expected_incident_id = std::env::var("TUNDRA_RECOVERY_INCIDENT_ID").ok();
    let mut handoff = read_handoff(
        argument_path.as_deref().or(environment_path.as_deref()),
        inline.as_deref(),
    )
    .unwrap_or_else(|error| RecoveryHandoffV1::generic(error.to_string()));
    if let Some(expected_incident_id) = expected_incident_id.as_deref() {
        handoff = handoff.bound_to_incident(expected_incident_id);
    }

    let incident_id = handoff.incident_id.clone();
    let mut status = run(handoff).unwrap_or(1);
    if status == RESTART_EXIT_CODE {
        let outcome_path = std::env::var_os("TUNDRA_RECOVERY_OUTCOME_PATH").map(PathBuf::from);
        if outcome_path
            .as_deref()
            .is_none_or(|path| write_restart_request(path, &incident_id).is_err())
        {
            status = 1;
        }
    }
    std::process::exit(status);
}
