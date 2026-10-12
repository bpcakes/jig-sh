use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::super::resume::*;
use super::super::*;

#[test]
fn session_ids_are_validated_and_normalized() {
    assert_eq!(
        normalize_session_id("019FE6E4-972F-7392-AAF3-58CB652A4E20").unwrap(),
        "019fe6e4-972f-7392-aaf3-58cb652a4e20"
    );
    for invalid in [
        "",
        "019fe6e4-972f-7392-aaf3-58cb652a4e2",
        "019fe6e4_972f-7392-aaf3-58cb652a4e20",
        "019fe6e4-972f-7392-aaf3-58cb652a4e2z",
    ] {
        assert!(normalize_session_id(invalid).is_err(), "accepted {invalid}");
    }

    let error = normalize_session_id("invalid\u{1b}[2Jsession")
        .unwrap_err()
        .to_string();
    assert!(error.contains("invalid"), "{error}");
    assert!(!error.contains('\u{1b}'), "{error}");
}

#[test]
fn resume_home_selection_requires_one_exact_match() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let discovered = || DiscoveredHomes {
        paths: vec![default.clone(), work.clone()],
        issues: Vec::new(),
        representation_lossy: false,
    };
    let thread_id = "019fe6e4-972f-7392-aaf3-58cb652a4e20";

    assert_eq!(
        select_resume_home(
            thread_id,
            discovered(),
            vec![ThreadHomeProbe::Missing, ThreadHomeProbe::Found],
        )
        .unwrap(),
        work.canonicalize().unwrap()
    );

    let error = select_resume_home(
        thread_id,
        discovered(),
        vec![ThreadHomeProbe::Found, ThreadHomeProbe::Found],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("exists in multiple homes"), "{error}");
    assert!(error.contains("pass --home HOME"), "{error}");

    let error = select_resume_home(
        thread_id,
        discovered(),
        vec![
            ThreadHomeProbe::Missing,
            ThreadHomeProbe::Failed(ResumeProbeFailure::Inspection(
                "app-server unavailable\u{1b}[2J".into(),
            )),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("could not be resolved"), "{error}");
    assert!(error.contains("app-server unavailable"), "{error}");
    assert!(!error.contains('\u{1b}'), "{error}");
}

#[test]
fn resume_home_probe_classification_captures_each_policy_outcome() {
    let homes = vec![
        PathBuf::from("/tmp/.codex"),
        PathBuf::from("/tmp/.codex-work"),
    ];

    match classify_resume_home(
        &homes,
        true,
        vec![ThreadHomeProbe::Missing, ThreadHomeProbe::Found],
    ) {
        ResumeHomeSelection::Unique(home) => assert_eq!(home, &homes[1]),
        selection => panic!("expected one confirmed match, got {selection:?}"),
    }

    match classify_resume_home(
        &homes,
        false,
        vec![ThreadHomeProbe::Found, ThreadHomeProbe::Missing],
    ) {
        ResumeHomeSelection::Unconfirmed { home, failures } => {
            assert_eq!(home, &homes[0]);
            assert!(failures.is_empty());
        }
        selection => panic!("expected unconfirmed match, got {selection:?}"),
    }

    match classify_resume_home(
        &homes,
        true,
        vec![ThreadHomeProbe::Found, ThreadHomeProbe::Found],
    ) {
        ResumeHomeSelection::Ambiguous(matches) => assert_eq!(matches, vec![&homes[0], &homes[1]]),
        selection => panic!("expected ambiguous matches, got {selection:?}"),
    }

    match classify_resume_home(
        &homes,
        true,
        vec![
            ThreadHomeProbe::Missing,
            ThreadHomeProbe::Failed(ResumeProbeFailure::Inspection(
                "app-server unavailable".into(),
            )),
        ],
    ) {
        ResumeHomeSelection::Missing {
            failures,
            discovery_incomplete,
        } => {
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].home, &homes[1]);
            assert_eq!(failures[0].failure.message(), "app-server unavailable");
            assert!(!discovery_incomplete);
        }
        selection => panic!("expected missing session, got {selection:?}"),
    }
}

#[test]
fn resume_home_selection_fails_closed_when_uniqueness_cannot_be_proven() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let thread_id = "019fe6e4-972f-7392-aaf3-58cb652a4e20";

    let error = select_resume_home(
        thread_id,
        DiscoveredHomes {
            paths: vec![default.clone(), work.clone()],
            issues: Vec::new(),
            representation_lossy: false,
        },
        vec![
            ThreadHomeProbe::Found,
            ThreadHomeProbe::Failed(ResumeProbeFailure::Inspection(
                "app-server unavailable".into(),
            )),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("uniqueness could not be confirmed"),
        "{error}"
    );
    assert!(error.contains("app-server unavailable"), "{error}");
    assert!(
        error.contains("some discovered homes could not be inspected"),
        "{error}"
    );
    assert!(error.contains("Pass --home HOME"), "{error}");
    assert!(!error.contains(&format!("Pass --home {}", default.display())));

    let error = select_resume_home(
        thread_id,
        DiscoveredHomes {
            paths: vec![default, work],
            issues: vec![DiscoveryIssue::new(
                DiscoveryIssueKind::ScanIncomplete,
                "candidate disappeared during discovery".into(),
            )],
            representation_lossy: false,
        },
        vec![ThreadHomeProbe::Found, ThreadHomeProbe::Missing],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("uniqueness could not be confirmed"),
        "{error}"
    );
    assert!(error.contains("home discovery was incomplete"), "{error}");
    assert!(
        !error.contains("some discovered homes could not be inspected"),
        "{error}"
    );
    assert!(error.contains("candidate disappeared"), "{error}");

    let error = select_resume_home(
        thread_id,
        DiscoveredHomes {
            paths: vec![PathBuf::from("/tmp/.codex")],
            issues: vec![DiscoveryIssue::new(
                DiscoveryIssueKind::ScanIncomplete,
                "home directory scan denied".into(),
            )],
            representation_lossy: false,
        },
        vec![ThreadHomeProbe::Missing],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("lookup coverage was incomplete"), "{error}");
    assert!(!error.contains("was not found"), "{error}");
}

#[test]
fn cancelled_resume_lookup_does_not_start_queued_probes() {
    let cancelled = AtomicBool::new(false);
    let probe_count = AtomicUsize::new(0);
    let homes = (0..4)
        .map(|index| PathBuf::from(format!("/tmp/.codex-{index}")))
        .collect::<Vec<_>>();

    let probes = probe_thread_homes_parallel_with_limit(
        &homes,
        &|| cancelled.load(Ordering::SeqCst),
        |_| {
            probe_count.fetch_add(1, Ordering::SeqCst);
            cancelled.store(true, Ordering::SeqCst);
            ThreadHomeProbe::Found
        },
        1,
    );

    assert_eq!(probe_count.load(Ordering::SeqCst), 1);
    assert!(matches!(probes.first(), Some(ThreadHomeProbe::Found)));
    assert!(probes[1..].iter().all(|probe| {
        matches!(
            probe,
            ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled)
        )
    }));

    cancelled.store(true, Ordering::SeqCst);
    let probes = probe_thread_homes_parallel(&homes, &|| true, |_| {
        probe_count.fetch_add(1, Ordering::SeqCst);
        ThreadHomeProbe::Found
    });

    assert_eq!(probe_count.load(Ordering::SeqCst), 1);
    assert_eq!(probes.len(), homes.len());
    assert!(probes.into_iter().all(|probe| {
        matches!(
            probe,
            ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled)
        )
    }));

    assert!(matches!(
        classify_resume_home(
            &homes,
            true,
            vec![
                ThreadHomeProbe::Found,
                ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled),
                ThreadHomeProbe::Missing,
                ThreadHomeProbe::Missing,
            ],
        ),
        ResumeHomeSelection::Cancelled
    ));

    let error = resolve_resume_home_selection(
        "019fe6e4-972f-7392-aaf3-58cb652a4e20",
        &DiscoveredHomes {
            paths: homes,
            issues: Vec::new(),
            representation_lossy: false,
        },
        ResumeHomeSelection::Cancelled,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, SESSION_LOOKUP_CANCELLED);
    assert!(!error.contains("--home"));
}

#[test]
fn resume_probe_progress_reports_initial_state_and_every_completion() {
    let homes = (0..3)
        .map(|index| PathBuf::from(format!("/tmp/.codex-{index}")))
        .collect::<Vec<_>>();
    let mut progress = Vec::new();

    let probes = probe_thread_homes_parallel_with_limit_and_progress(
        &homes,
        &|| false,
        |_| ThreadHomeProbe::Missing,
        2,
        |completed, total| progress.push((completed, total)),
    );

    assert_eq!(progress, vec![(0, 3), (1, 3), (2, 3), (3, 3)]);
    assert!(
        probes
            .into_iter()
            .all(|probe| matches!(probe, ThreadHomeProbe::Missing))
    );
}

#[test]
fn resume_probe_limit_of_zero_still_probes_each_home() {
    let homes = (0..3)
        .map(|index| PathBuf::from(format!("/tmp/.codex-{index}")))
        .collect::<Vec<_>>();
    let probe_count = AtomicUsize::new(0);

    let probes = probe_thread_homes_parallel_with_limit(
        &homes,
        &|| false,
        |_| {
            probe_count.fetch_add(1, Ordering::SeqCst);
            ThreadHomeProbe::Missing
        },
        0,
    );

    assert_eq!(probe_count.load(Ordering::SeqCst), homes.len());
    assert!(
        probes
            .into_iter()
            .all(|probe| matches!(probe, ThreadHomeProbe::Missing))
    );
}

#[test]
fn panicking_resume_probe_keeps_its_domain_specific_failure() {
    let homes = vec![PathBuf::from("/tmp/.codex")];

    let probes = probe_thread_homes_parallel_with_limit(
        &homes,
        &|| false,
        |_| panic!("simulated resume-probe panic"),
        1,
    );

    assert!(matches!(
        probes.first(),
        Some(ThreadHomeProbe::Failed(ResumeProbeFailure::WorkerPanicked))
    ));
}
