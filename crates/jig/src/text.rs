pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        truncated.push('…');
    }
    truncated
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncates_at_character_boundaries_with_one_ellipsis() {
        assert_eq!(truncate_chars("aé日", 2), "aé…");
        assert_eq!(truncate_chars("aé", 2), "aé");
        assert_eq!(truncate_chars("é", 0), "…");
    }
}
