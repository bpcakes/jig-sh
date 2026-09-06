#[test]
fn status_entrypoint_uses_requested_refresh_cadence() {
    let interval = std::time::Duration::from_secs(3_600);
    let options = super::status_dashboard_options(interval);

    assert_eq!(options.refresh_interval, interval);
}

#[test]
fn work_options_preserve_cli_refresh_limit_and_initial_plan() {
    let options = super::work_dashboard_options(
        crate::cli::UiOpts {
            refresh_seconds: Some(11),
            timeline_limit: Some(250),
            plan: Some("plan_example".to_string()),
            retired_port: None,
        },
        jig_ui::dashboard::TimelineLimit::new(250).unwrap(),
    )
    .unwrap();

    assert_eq!(options.initial_tab, jig_ui::terminal::InitialTab::Work);
    assert_eq!(options.refresh_interval, std::time::Duration::from_secs(11));
    assert_eq!(options.timeline_limit.get(), 250);
    assert_eq!(options.initial_plan.as_deref(), Some("plan_example"));
}

#[test]
fn errors_after_json_output_are_marked_as_already_emitted() {
    let error =
        super::finish_json_result(Err(anyhow::anyhow!("retirement failed")), true).unwrap_err();
    assert!(crate::cli::is_json_output_already_emitted(&error));

    let pre_output =
        super::finish_json_result(Err(anyhow::anyhow!("collection failed")), false).unwrap_err();
    assert!(!crate::cli::is_json_output_already_emitted(&pre_output));
}

#[test]
fn a_partial_json_write_failure_is_never_followed_by_an_error_document() {
    struct PartialWriter(bool);

    impl std::io::Write for PartialWriter {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            if self.0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "closed output",
                ));
            }
            self.0 = true;
            Ok(input.len().min(1))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let write = super::write_json_to(&mut PartialWriter(false), b"{\"ok\":true}\n");
    let error = super::finish_json_result(write, true).unwrap_err();
    assert!(crate::cli::is_json_output_already_emitted(&error));
}
