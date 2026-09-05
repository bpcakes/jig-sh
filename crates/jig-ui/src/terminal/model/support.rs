pub(super) fn moved_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    current
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}
