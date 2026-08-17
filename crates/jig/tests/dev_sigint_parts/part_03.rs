fn terminate_verified_group(identity: &VerifiedProcessIdentity) {
    if !identity.owns_process_group() {
        return;
    }
    let _ = unsafe {
        // SAFETY: getpgid and the process start token verified that this
        // negative pid still names the helper-owned process group.
        libc::kill(-identity.pid, libc::SIGTERM)
    };
    thread::sleep(Duration::from_millis(250));
    if identity.owns_process_group() {
        let _ = unsafe {
            // SAFETY: the group leader's pinned identity was reverified above.
            libc::kill(-identity.pid, libc::SIGKILL)
        };
    }
    let _ = wait_for_verified_exit(identity, Duration::from_secs(2));
}
