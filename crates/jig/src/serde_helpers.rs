use serde::{Deserialize, Deserializer};

pub(crate) fn null_or_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

pub(crate) const fn default_receipts_limit() -> usize {
    crate::tool_defs::DEFAULT_RECEIPTS_LIMIT
}

pub(crate) fn null_as_default_receipts_limit<'de, D>(
    deserializer: D,
) -> std::result::Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<usize>::deserialize(deserializer)
        .map(|value| value.unwrap_or(crate::tool_defs::DEFAULT_RECEIPTS_LIMIT))
}
