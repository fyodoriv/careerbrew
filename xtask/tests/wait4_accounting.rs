#![cfg(any(target_os = "macos", target_os = "linux"))]
pub mod capture_actions {
    include!("../src/capture_actions.rs");

    impl OwnerProbe {
        fn snapshot(&self) -> Value {
            let terminal = self.0.terminal.borrow();
            json!({"pid":self.0.pid.get(),"native_attempts":self.0.wait_attempts.get(),"native_consuming_reaps":self.0.native_reaps.get(),"group_signals":self.get(),"wait_observed":terminal.is_some(),"reaped":terminal.is_some(),"cleanup_succeeded":terminal.is_some(),"terminal":terminal.as_ref().map(|(code,signal,usage)|json!({"code":code,"signal":signal,"raw":usage.0.as_ref().map(|r|format!("{r:?}")),"validation":format!("{:?}",usage.1)})),"resource_admitted":false})
        }
    }

    #[cfg(test)]
    #[derive(Clone, Copy, Default)]
    struct SyntheticRawUsage {
        user_sec: i64,
        user_usec: i64,
        system_sec: i64,
        system_usec: i64,
        max_rss: i64,
    }
    #[cfg(test)]
    #[derive(Clone, Copy)]
    enum TestRssUnit {
        Bytes,
        KiB,
        Unknown,
    }
    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq)]
    struct UnitCandidate {
        cpu_ns: u64,
        max_rss_bytes: u64,
    }
    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq)]
    enum UnitError {
        NegativeTime,
        InvalidTimeval,
        NegativeRss,
        ArithmeticOverflow,
        UnsupportedUnit,
        MissingUsage,
    }

    #[cfg(test)]
    fn checked_units(
        input: Option<&SyntheticRawUsage>,
        unit: TestRssUnit,
    ) -> Result<UnitCandidate, UnitError> {
        use UnitError::*;
        let raw = input.ok_or(MissingUsage)?;
        if raw.user_sec < 0 || raw.system_sec < 0 {
            return Err(NegativeTime);
        }
        if !(0..1_000_000).contains(&raw.user_usec) || !(0..1_000_000).contains(&raw.system_usec) {
            return Err(InvalidTimeval);
        }
        if raw.max_rss < 0 {
            return Err(NegativeRss);
        }
        let seconds = (raw.user_sec as u64)
            .checked_add(raw.system_sec as u64)
            .ok_or(ArithmeticOverflow)?;
        let usecs = (raw.user_usec as u64)
            .checked_add(raw.system_usec as u64)
            .ok_or(ArithmeticOverflow)?;
        let cpu_ns = seconds
            .checked_mul(1_000_000_000)
            .and_then(|s| usecs.checked_mul(1000).and_then(|u| s.checked_add(u)))
            .ok_or(ArithmeticOverflow)?;
        let max_rss_bytes = match unit {
            TestRssUnit::Bytes => raw.max_rss as u64,
            TestRssUnit::KiB => (raw.max_rss as u64)
                .checked_mul(1024)
                .ok_or(ArithmeticOverflow)?,
            TestRssUnit::Unknown => return Err(UnsupportedUnit),
        };
        Ok(UnitCandidate {
            cpu_ns,
            max_rss_bytes,
        })
    }

    fn record_case(
        id: &str,
        check: impl FnOnce(&OwnerProbe) -> serde_json::Value,
    ) -> serde_json::Value {
        let probe = OwnerProbe::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&probe)));
        let observation = probe.snapshot();
        match result {
            Ok(evidence) => {
                let ok = evidence
                    .get("subcases")
                    .and_then(Value::as_array)
                    .is_none_or(|rows| rows.iter().all(|row| row["ok"] == true));
                json!({"id":id,"ok":ok,"observation":observation,"evidence":evidence})
            }
            Err(_) => {
                json!({"id":id,"ok":false,"observation":observation,"refusal":"control assertion failed"})
            }
        }
    }

    fn retain(kind: &str, rows: &[serde_json::Value]) {
        use std::os::unix::fs::OpenOptionsExt;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        std::fs::create_dir_all(root.join(".metrics")).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("wait4-phase-a-")
            .tempdir_in(root.join(".metrics"))
            .unwrap()
            .keep();
        let bytes = serde_json::to_vec_pretty(
            &json!({"scope":"phase-A only; no resource admission","kind":kind,"rows":rows}),
        )
        .unwrap();
        assert!(bytes.len() <= 128 * 1024);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("outcomes.json"))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        println!("phase-A evidence: {}", directory.display());
        assert!(rows.iter().all(|row| row["ok"] == true), "{rows:#?}");
    }

    fn terminal(child: &mut Child) {
        let end = Instant::now() + Duration::from_secs(5);
        while !leader_exited(child).unwrap() {
            assert!(Instant::now() < end, "owned actor did not terminate");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn absent(probe: &OwnerProbe, pid: u32) -> serde_json::Value {
        assert_eq!(probe.0.pid.get(), Some(pid));
        assert!(pid > 0);
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let wait = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        let wait_error = std::io::Error::last_os_error().raw_os_error();
        let signal = unsafe { libc::kill(pid as i32, 0) };
        let signal_error = std::io::Error::last_os_error().raw_os_error();
        assert_eq!((wait, wait_error), (-1, Some(libc::ECHILD)));
        assert_eq!((signal, signal_error), (-1, Some(libc::ESRCH)));
        json!({"pid":pid,"nonconsuming_probe":"ECHILD","absence":"ESRCH"})
    }

    fn counters(probe: &OwnerProbe, consumes: usize) -> serde_json::Value {
        assert!(probe.0.wait_attempts.get() >= consumes);
        if consumes == 0 {
            assert_eq!(probe.0.wait_attempts.get(), 0);
        }
        assert_eq!(probe.0.native_reaps.get(), consumes);
        json!({"native_attempts":probe.0.wait_attempts.get(),"native_consuming_reaps":probe.0.native_reaps.get(),"group_signals":probe.get(),"resource_admitted":false})
    }

    fn command(path: &str, args: &[&str]) -> Command {
        let mut command = Command::new(path);
        command.args(args).env_clear();
        command
    }

    fn guard(path: &str, args: &[&str], probe: &OwnerProbe) -> (RunningChild, tempfile::TempDir) {
        let streams = tempfile::tempdir().unwrap();
        let mut actor = command(path, args);
        actor
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(File::create(streams.path().join("stdout")).unwrap())
            .stderr(File::create(streams.path().join("stderr")).unwrap());
        let child = actor.spawn().unwrap();
        probe.0.pid.set(Some(child.id()));
        (
            RunningChild {
                child,
                released: false,
                group_signals: probe.clone(),
            },
            streams,
        )
    }

    fn empty_streams(streams: &tempfile::TempDir) {
        for name in ["stdout", "stderr"] {
            assert_eq!(bounded(&streams.path().join(name)).unwrap(), "");
        }
    }

    fn assert_injected_field(raw: &RawWaitUsage, issue: RawUsageIssue) {
        let (user_sec, system_usec, max_rss) = match *raw {
            #[cfg(target_os = "macos")]
            RawWaitUsage::MacOSWait4BytesUncalibrated {
                user_sec,
                system_usec,
                max_rss,
                ..
            } => (user_sec, system_usec, max_rss),
            #[cfg(target_os = "linux")]
            RawWaitUsage::LinuxWait4KiBUncalibrated {
                user_sec,
                system_usec,
                max_rss,
                ..
            } => (user_sec, system_usec, max_rss),
        };
        match issue {
            RawUsageIssue::NegativeTime => assert_eq!(user_sec, -1),
            RawUsageIssue::InvalidTimeval => assert_eq!(system_usec, 1_000_000),
            RawUsageIssue::NegativeRss => assert_eq!(max_rss, -1),
        }
    }

    fn invalid_time(raw: &mut libc::rusage) {
        raw.ru_utime.tv_sec = -1;
    }
    fn invalid_usec(raw: &mut libc::rusage) {
        raw.ru_stime.tv_usec = 1_000_000;
    }
    fn invalid_rss(raw: &mut libc::rusage) {
        raw.ru_maxrss = -1;
    }

    #[test]
    fn owned_wait4_actual_lifecycle() {
        let mut rows = Vec::new();
        for (id, path, args, code, signal) in [
            ("L01", "/usr/bin/true", &[][..], Some(0), None),
            ("L02", "/bin/sh", &["-c", "exit 17"][..], Some(17), None),
            (
                "L03",
                "/bin/sh",
                &["-c", "kill -TERM $$"][..],
                None,
                Some(libc::SIGTERM),
            ),
        ] {
            rows.push(record_case(id, |probe| {
                let result = execute_during_observed_inner(command(path,args), Duration::from_secs(5), |_,_| Ok(()), Some(probe.clone())).unwrap();
                assert_eq!(result.observation.wait_code,code);
                assert_eq!(result.observation.wait_signal,signal);
                assert!(result.observation.wait_observed && result.observation.reaped && result.observation.cleanup_succeeded);
                assert_eq!((result.stdout.as_str(),result.stderr.as_str()),("",""));
                json!({"owner":counters(probe,1),"absence":absent(probe,result.observation.pid),"code":code,"signal":signal})
            }));
        }
        rows.push(record_case("L04", |probe| {
            let (mut owned, streams) = guard("/usr/bin/true", &[], probe);
            let pid = owned.child.id();
            terminal(&mut owned.child);
            assert!(leader_exited(&mut owned.child).unwrap());
            let reaped = owned.stop_and_reap().unwrap();
            assert!(reaped.success());
            let (status, usage) = reaped.into_parts();
            assert_eq!(status.code(), Some(0));
            assert!(usage.0.is_some() && usage.1.is_ok());
            assert_eq!(
                owned.stop_and_reap().unwrap_err().raw_os_error(),
                Some(libc::ECHILD)
            );
            drop(owned);
            empty_streams(&streams);
            assert_eq!(probe.get(), 1);
            json!({"owner":counters(probe,1),"absence":absent(probe,pid),"terminal_nonconsuming_polls":2})
        }));
        for (id, deadline, callback) in [
            ("L05", Duration::from_millis(20), false),
            ("L06", Duration::from_secs(5), true),
        ] {
            rows.push(record_case(id, |probe| {
                let failure = execute_during_observed_inner(command("/bin/sleep", &["30"]),deadline,|_,_| if callback {Err("synthetic callback refusal".into())} else {Ok(())},Some(probe.clone())).unwrap_err();
                assert_eq!(failure.cause,if callback {OwnerFailureCause::Callback} else {OwnerFailureCause::Deadline});
                let observed = failure.observation.unwrap();
                assert_eq!((observed.wait_code,observed.wait_signal),(None,Some(libc::SIGKILL)));
                assert!(observed.wait_observed && observed.reaped && observed.cleanup_succeeded);
                json!({"owner":counters(probe,1),"absence":absent(probe,observed.pid),"cause":format!("{:?}",failure.cause)})
            }));
        }
        rows.push(record_case("L07", |probe| {
            use std::os::unix::ffi::OsStrExt;
            let mut actor = command("/usr/bin/printf", &[]);
            actor.arg(std::ffi::OsStr::from_bytes(&[0x5c,0x33,0x37,0x37]));
            let failure = execute_during_observed_inner(actor,Duration::from_secs(5),|_,_|Ok(()),Some(probe.clone())).unwrap_err();
            assert_eq!(failure.cause,OwnerFailureCause::Output);
            let observed = failure.observation.unwrap();
            assert_eq!(observed.wait_code,Some(0));
            assert!(observed.wait_observed && observed.reaped && observed.cleanup_succeeded);
            json!({"owner":counters(probe,1),"absence":absent(probe,observed.pid),"arg_hex":"5c333737","cause":"Output"})
        }));
        rows.push(record_case("L08", |probe| {
            let (owned, streams) = guard("/bin/sleep", &["30"], probe);
            let pid = owned.child.id();
            drop(owned);
            empty_streams(&streams);
            assert_eq!(probe.get(), 1);
            json!({"owner":counters(probe,1),"absence":absent(probe,pid)})
        }));
        rows.push(record_case("L09", |probe| {
            let (mut owned, streams) = guard("/usr/bin/true", &[], probe);
            let pid = owned.child.id();
            terminal(&mut owned.child);
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
                pid as i32
            );
            assert_eq!(
                owned.stop_and_reap().unwrap_err().raw_os_error(),
                Some(libc::ECHILD)
            );
            drop(owned);
            empty_streams(&streams);
            assert_eq!(probe.get(), 0);
            json!({"owner":counters(probe,0),"absence":absent(probe,pid),"external_consuming_reaps":1})
        }));
        rows.push(record_case("L14", |_probe| {
            let mut results = Vec::new();
            for (fault,expected) in [(invalid_time as fn(&mut libc::rusage),RawUsageIssue::NegativeTime),(invalid_usec,RawUsageIssue::InvalidTimeval),(invalid_rss,RawUsageIssue::NegativeRss)] {
                results.push(record_case(&format!("{expected:?}"), |probe| {
                    probe.0.raw_fault.set(Some(fault));
                    let result = execute_during_observed_inner(command("/usr/bin/true", &[]),Duration::from_secs(5),|_,_|Ok(()),Some(probe.clone())).unwrap();
                    assert_eq!(result.observation.wait_code,Some(0));
                    assert!(result.observation.wait_observed && result.observation.reaped && result.observation.cleanup_succeeded);
                    assert_eq!((result.stdout.as_str(),result.stderr.as_str()),("",""));
                    let (_,_,usage)=probe.0.terminal.borrow().as_ref().unwrap().clone();
                    assert_eq!(usage.1,Err(expected));
                    let raw=usage.0.unwrap();assert_eq!(validate_raw(&raw),Err(expected));assert_injected_field(&raw,expected);
                    json!({"owner":counters(probe,1),"absence":absent(probe,result.observation.pid),"raw":format!("{raw:?}"),"unavailable":format!("{expected:?}"),"code":0})
                }));
            }
            json!({"subcases":results,"all_three_required":true})
        }));
        rows.push(record_case("L15", |probe| {
            let text="{\"cpu_ns\":-1,\"max_rss_bytes\":-1}";
            let result=execute_during_observed_inner(command("/bin/echo",&[text]),Duration::from_secs(5),|_,_|Ok(()),Some(probe.clone())).unwrap();
            assert_eq!(result.stdout,format!("{text}\n"));assert_eq!(result.observation.wait_code,Some(0));
            json!({"owner":counters(probe,1),"absence":absent(probe,result.observation.pid),"stdout_is_ordinary_output":true})
        }));
        assert_eq!(rows.len(), 11);
        retain("native-lifecycle", &rows);
    }

    #[test]
    fn owned_wait4_scripted_contract() {
        let mut rows = Vec::new();
        rows.push(record_case("L10", |_probe| {
            let mut released = false;
            let mut context = ReapContext {
                expected_pid: 424242,
                released: &mut released,
                raw_fault: None,
            };
            assert!(matches!(
                finish_wait_attempt(&mut context, WaitAttempt::Interrupted),
                ReapDecision::Retry
            ));
            assert!(!*context.released);
            let usage = unsafe { std::mem::zeroed() };
            let ReapDecision::Done(result) = finish_wait_attempt(
                &mut context,
                WaitAttempt::Returned {
                    pid: 424242,
                    raw_status: 0,
                    rusage: usage,
                },
            ) else {
                panic!("terminal attempt retried")
            };
            assert_eq!(result.unwrap().into_parts().0.code(), Some(0));
            assert!(*context.released);
            json!({"scripted_attempts":2,"reported_terminal_results":1,"native_consuming_reaps":0})
        }));
        for (id, attempt, kind) in [
            ("L11", WaitAttempt::Failed(libc::EIO), "EIO"),
            (
                "L12",
                WaitAttempt::Returned {
                    pid: 424243,
                    raw_status: 0,
                    rusage: unsafe { std::mem::zeroed() },
                },
                "wrong PID",
            ),
            (
                "L13",
                WaitAttempt::Returned {
                    pid: 424242,
                    raw_status: (libc::SIGSTOP << 8) | 0x7f,
                    rusage: unsafe { std::mem::zeroed() },
                },
                "nonterminal",
            ),
        ] {
            rows.push(record_case(id,|_probe| {
                let mut released=false;let mut context=ReapContext {expected_pid:424242,released:&mut released,raw_fault:None};
                let ReapDecision::Done(result)=finish_wait_attempt(&mut context,attempt) else {panic!("final refusal retried")};
                let error=result.unwrap_err();assert!(*context.released);
                if id=="L11" {assert_eq!(error.raw_os_error(),Some(libc::EIO));} else {assert_eq!(error.kind(),std::io::ErrorKind::InvalidData);}
                json!({"scripted_attempts":1,"native_consuming_reaps":0,"refusal":kind,"released":released})
            }));
        }
        assert_eq!(rows.len(), 4);
        retain("scripted-transitions", &rows);
    }

    #[test]
    fn checked_resource_arithmetic() {
        use TestRssUnit::{Bytes, KiB, Unknown};
        use UnitError::*;
        let zero = SyntheticRawUsage::default();
        let cases = [
            (
                "U01",
                Some(zero),
                Bytes,
                Ok(UnitCandidate {
                    cpu_ns: 0,
                    max_rss_bytes: 0,
                }),
            ),
            (
                "U02",
                Some(SyntheticRawUsage {
                    user_sec: 1,
                    user_usec: 2,
                    system_sec: 3,
                    system_usec: 4,
                    max_rss: 17,
                }),
                Bytes,
                Ok(UnitCandidate {
                    cpu_ns: 4_000_006_000,
                    max_rss_bytes: 17,
                }),
            ),
            (
                "U03",
                Some(SyntheticRawUsage {
                    user_usec: 999999,
                    system_usec: 999999,
                    max_rss: 1,
                    ..zero
                }),
                KiB,
                Ok(UnitCandidate {
                    cpu_ns: 1_999_998_000,
                    max_rss_bytes: 1024,
                }),
            ),
            (
                "U04",
                Some(SyntheticRawUsage {
                    user_sec: 18446744073,
                    user_usec: 709551,
                    ..zero
                }),
                Bytes,
                Ok(UnitCandidate {
                    cpu_ns: 18_446_744_073_709_551_000,
                    max_rss_bytes: 0,
                }),
            ),
            (
                "U05",
                Some(SyntheticRawUsage {
                    user_sec: 18446744073,
                    user_usec: 709552,
                    ..zero
                }),
                Bytes,
                Err(ArithmeticOverflow),
            ),
            (
                "U06",
                Some(SyntheticRawUsage {
                    max_rss: 18014398509481983,
                    ..zero
                }),
                KiB,
                Ok(UnitCandidate {
                    cpu_ns: 0,
                    max_rss_bytes: 18_446_744_073_709_550_592,
                }),
            ),
            (
                "U07",
                Some(SyntheticRawUsage {
                    max_rss: 18014398509481984,
                    ..zero
                }),
                KiB,
                Err(ArithmeticOverflow),
            ),
            (
                "U08",
                Some(SyntheticRawUsage {
                    max_rss: i64::MAX,
                    ..zero
                }),
                Bytes,
                Ok(UnitCandidate {
                    cpu_ns: 0,
                    max_rss_bytes: 9_223_372_036_854_775_807,
                }),
            ),
            (
                "U09",
                Some(SyntheticRawUsage {
                    user_sec: -1,
                    ..zero
                }),
                Bytes,
                Err(NegativeTime),
            ),
            (
                "U10",
                Some(SyntheticRawUsage {
                    system_sec: -1,
                    ..zero
                }),
                Bytes,
                Err(NegativeTime),
            ),
            (
                "U11",
                Some(SyntheticRawUsage {
                    user_usec: -1,
                    ..zero
                }),
                Bytes,
                Err(InvalidTimeval),
            ),
            (
                "U12",
                Some(SyntheticRawUsage {
                    system_usec: -1,
                    ..zero
                }),
                Bytes,
                Err(InvalidTimeval),
            ),
            (
                "U13",
                Some(SyntheticRawUsage {
                    user_usec: 1000000,
                    ..zero
                }),
                Bytes,
                Err(InvalidTimeval),
            ),
            (
                "U14",
                Some(SyntheticRawUsage {
                    system_usec: 1000000,
                    ..zero
                }),
                Bytes,
                Err(InvalidTimeval),
            ),
            (
                "U15",
                Some(SyntheticRawUsage {
                    max_rss: -1,
                    ..zero
                }),
                Bytes,
                Err(NegativeRss),
            ),
            ("U16", Some(zero), Unknown, Err(UnsupportedUnit)),
            (
                "U17",
                Some(SyntheticRawUsage {
                    user_sec: i64::MAX,
                    system_sec: i64::MAX,
                    ..zero
                }),
                Bytes,
                Err(ArithmeticOverflow),
            ),
            ("U18", None, Bytes, Err(MissingUsage)),
        ];
        let rows:Vec<_>=cases.into_iter().map(|(id,input,unit,expected)| record_case(id,|_probe| {
            let actual=checked_units(input.as_ref(),unit);assert_eq!(actual,expected);
            json!({"actual":format!("{actual:?}"),"synthetic_only":true,"native_consuming_reaps":0})
        })).collect();
        assert_eq!(rows.len(), 18);
        retain("checked-arithmetic", &rows);
    }
}
