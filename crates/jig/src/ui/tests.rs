#[test]
fn status_entrypoint_uses_requested_refresh_cadence() {
    let interval = std::time::Duration::from_secs(3_600);
    let options = super::status_dashboard_options(interval);

    assert_eq!(options.refresh_interval, interval);
}

#[test]
fn ui_options_start_on_timeline_with_requested_refresh_and_limit() {
    let options = super::timeline_dashboard_options(super::DashboardRequest {
        timeline_limit: 250,
        refresh_interval: std::time::Duration::from_secs(11),
    })
    .unwrap();

    assert_eq!(options.initial_tab, jig_ui::terminal::InitialTab::Timeline);
    assert_eq!(options.refresh_interval, std::time::Duration::from_secs(11));
    assert_eq!(options.timeline_limit.get(), 250);
}

#[test]
fn recorder_json_failures_report_whether_output_started() {
    let after_output =
        super::recorder_json_result(Err(anyhow::anyhow!("retirement failed")), true).unwrap_err();
    assert!(after_output.output_started);
    assert_eq!(after_output.error.to_string(), "retirement failed");

    let pre_output =
        super::recorder_json_result(Err(anyhow::anyhow!("collection failed")), false).unwrap_err();
    assert!(!pre_output.output_started);
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
    let failure = super::recorder_json_result(write, true).unwrap_err();
    assert!(failure.output_started);
}
