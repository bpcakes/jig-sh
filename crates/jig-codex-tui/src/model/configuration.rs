use jig_tui::sanitize_text;

use super::App;
use crate::ConfigurationHome;

impl App {
    pub(crate) fn configuration(
        title: &str,
        homes: Vec<ConfigurationHome>,
        warnings: Vec<String>,
    ) -> Self {
        let (homes, details): (Vec<_>, Vec<_>) = homes
            .into_iter()
            .map(|entry| (entry.home, entry.details))
            .unzip();
        let mut app = Self::new(homes, warnings);
        app.configuration_title = Some(sanitize_text(title));
        app.static_configuration = true;
        app.inspection_finished = true;
        app.completed = app.rows.len();
        for (row, details) in app.rows.iter_mut().zip(details) {
            row.configuration_details = Some(
                details
                    .into_iter()
                    .map(|(label, value)| (sanitize_text(&label), sanitize_text(&value)))
                    .collect(),
            );
        }
        app
    }

    pub(crate) fn inspected_configuration(
        title: &str,
        homes: Vec<ConfigurationHome>,
        warnings: Vec<String>,
    ) -> Self {
        let mut app = Self::configuration(title, homes, warnings);
        app.static_configuration = false;
        app.inspection_finished = false;
        app.completed = 0;
        app
    }

    pub(crate) fn title(&self) -> &str {
        self.configuration_title
            .as_deref()
            .unwrap_or("Codex Home Picker")
    }
}
