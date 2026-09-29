//! Backend-neutral migration command DTOs.

#[derive(Debug)]
pub(crate) struct MigrationAddRequest {
    pub(crate) name: String,
}
