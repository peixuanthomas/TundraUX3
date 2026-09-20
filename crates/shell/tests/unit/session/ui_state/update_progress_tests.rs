use super::*;
use app::update::{UpdatePhase, UpdateProgress, UpdateProgressDetail};

#[test]
fn update_progress_retains_meters_and_bounds_live_output() {
    let mut state = SettingsUpdateState::default();
    state.apply_progress(UpdateProgress {
        phase: UpdatePhase::Compiling,
        message: "Compiling".into(),
        detail: UpdateProgressDetail::Compilation {
            completed: 10,
            total: Some(10),
            finished: false,
        },
    });
    assert_eq!(
        state.activity.as_ref().unwrap().compilation.percent,
        Some(99)
    );
    for n in 0..250 {
        state.apply_progress(UpdateProgress {
            phase: UpdatePhase::Compiling,
            message: format!("output {n}"),
            detail: UpdateProgressDetail::Output,
        });
    }
    assert_eq!(state.status.render_current(), "Compiling");
    assert_eq!(state.activity.as_ref().unwrap().output.len(), 200);
    assert_eq!(
        state.activity.as_ref().unwrap().output.last().unwrap(),
        "output 249"
    );
    state.append_output("ERROR: compiler failed");
    assert_eq!(
        state.activity.as_ref().unwrap().compilation.percent,
        Some(99)
    );
    state.apply_progress(UpdateProgress {
        phase: UpdatePhase::Compiling,
        message: "Complete".into(),
        detail: UpdateProgressDetail::Compilation {
            completed: 1,
            total: Some(1),
            finished: true,
        },
    });
    assert_eq!(
        state.activity.as_ref().unwrap().compilation.percent,
        Some(100)
    );
}
