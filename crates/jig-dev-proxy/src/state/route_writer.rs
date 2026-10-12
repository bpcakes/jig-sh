#[cfg(test)]
use std::cell::Cell;
use std::io::Write;
use std::path::Path;

use anyhow::Result;

use super::{ROUTES_VERSION, Route, RoutesDocument};
use crate::file_ops;

#[cfg(test)]
thread_local! {
    static FAIL_ROUTE_WRITE_ONCE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn fail_route_write_once() {
    FAIL_ROUTE_WRITE_ONCE.with(|flag| flag.set(true));
}

pub(super) fn write_routes_to_path(path: &Path, routes: &[Route]) -> Result<()> {
    #[cfg(test)]
    if FAIL_ROUTE_WRITE_ONCE.with(|flag| flag.replace(false)) {
        anyhow::bail!("injected route write failure before replace");
    }
    let tmp = file_ops::temp_path(path, "jig-proxy-state");
    let mut file = file_ops::create_new_file(&tmp, 0o600)?;
    serde_json::to_writer_pretty(
        &mut file,
        &RoutesDocument {
            version: ROUTES_VERSION,
            routes,
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path)?;
    Ok(())
}
