mod pipe;
mod process;

pub mod unix;

pub use pipe::{ChildPipe, NonblockingPipe};
pub use process::interaction;
pub use process::{
    BoundedProcessOutput, OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    OwnedProcessTreeOutput, ProcessOutputLimits, ProcessOutputOverflowPolicy, format_exit_status,
    require_success, run_checked_output, run_checked_output_with_context,
    run_checked_stdout_trimmed, run_owned_process_tree_with_output,
    run_owned_process_tree_with_output_limits,
    run_owned_process_tree_with_output_limits_and_observer,
    run_owned_process_tree_with_output_policy_and_observer,
};

#[cfg(test)]
mod test_env;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod test_process;
