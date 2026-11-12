#[allow(dead_code)]
mod capture_actions {
    include!("../../src/capture_actions.rs");
    pub fn limited(
        c: std::process::Command,
        d: std::time::Duration,
    ) -> Result<(u32, Option<i32>, String, String), String> {
        execute_bounded(c, d)
    }
}
pub fn invalid_budget_never_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("spawned");
    for duration in [
        std::time::Duration::ZERO,
        std::time::Duration::from_secs(31),
    ] {
        let mut c = std::process::Command::new("/usr/bin/touch");
        c.arg(&marker);
        assert!(capture_actions::limited(c, duration)
            .unwrap_err()
            .contains("budget"));
        assert!(!marker.exists());
        let mut command = std::process::Command::new("/usr/bin/touch");
        command.arg(&marker);
        let failure =
            capture_actions::execute_during_observed(command, duration, |_, _| Ok(())).unwrap_err();
        assert_eq!(
            failure.cause,
            capture_actions::OwnerFailureCause::InvalidBudget
        );
        assert!(failure.observation.is_none());
        assert!(!marker.exists());
    }
}
pub fn positive_budget_preserves_actual_exit() {
    let c = std::process::Command::new("/usr/bin/true");
    assert_eq!(
        capture_actions::limited(c, std::time::Duration::from_secs(1))
            .unwrap()
            .1,
        Some(0)
    );

    let run = capture_actions::execute_during_observed(
        std::process::Command::new("/usr/bin/true"),
        std::time::Duration::from_secs(5),
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(run.observation.wait_code, Some(0));
    assert_eq!(run.observation.wait_signal, None);
    assert!(
        run.observation.wait_observed
            && run.observation.reaped
            && run.observation.cleanup_succeeded
    );
    let failure = capture_actions::execute_during_observed(
        std::process::Command::new("/usr/bin/true"),
        std::time::Duration::from_secs(5),
        |_, check| {
            while !check()? {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err("synthetic callback rejection".into())
        },
    )
    .unwrap_err();
    assert_eq!(failure.cause, capture_actions::OwnerFailureCause::Callback);
    let observed = failure.observation.unwrap();
    assert_eq!(observed.wait_code, Some(0));
    assert_eq!(observed.wait_signal, None);
    assert!(observed.wait_observed && observed.reaped && observed.cleanup_succeeded);
    let mut command = std::process::Command::new("/usr/bin/printf");
    command.arg("\\377");
    let failure = capture_actions::execute_during_observed(
        command,
        std::time::Duration::from_secs(5),
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(failure.cause, capture_actions::OwnerFailureCause::Output);
    let observed = failure.observation.unwrap();
    assert_eq!(observed.wait_code, Some(0));
    assert!(observed.wait_observed && observed.reaped && observed.cleanup_succeeded);
}
pub fn short_budget_kills_and_reaps_actual_child() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg("echo $$ > \"$1\"; exec /bin/sleep 5")
        .arg("owned")
        .arg(&pidfile);
    let error = capture_actions::limited(c, std::time::Duration::from_millis(100)).unwrap_err();
    assert!(error.contains("timeout"));
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );

    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("5");
    let failure = capture_actions::execute_during_observed(
        command,
        std::time::Duration::from_millis(100),
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(failure.cause, capture_actions::OwnerFailureCause::Deadline);
    let observed = failure.observation.unwrap();
    assert_eq!(observed.program, std::ffi::OsStr::new("/bin/sleep"));
    assert_eq!(observed.args, vec![std::ffi::OsString::from("5")]);
    assert_eq!(observed.wait_code, None);
    assert_eq!(observed.wait_signal, Some(libc::SIGKILL));
    assert!(observed.wait_observed && observed.reaped && observed.cleanup_succeeded);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(observed.pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert_eq!(unsafe { libc::kill(observed.pid as i32, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
