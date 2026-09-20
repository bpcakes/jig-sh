use std::{
    env, fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn main() {
    println!("cargo:rerun-if-env-changed=PROBE_CONDITION");
    let Some(control) = env::var_os("PROBE_CONTROL") else {
        return;
    };
    let control = Path::new(&control);
    fs::write(control.join("build-ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !control.join("release").exists() {
        assert!(Instant::now() < deadline, "probe release deadline expired");
        thread::sleep(Duration::from_millis(10));
    }
}
