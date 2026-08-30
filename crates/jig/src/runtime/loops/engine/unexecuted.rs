#[derive(Debug)]
pub(in crate::runtime::loops) struct UnexecutedTickError(anyhow::Error);

impl UnexecutedTickError {
    pub(in crate::runtime::loops) fn into_inner(self) -> anyhow::Error {
        self.0
    }
}

impl std::fmt::Display for UnexecutedTickError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if formatter.alternate() {
            write!(formatter, "{:#}", self.0)
        } else {
            write!(formatter, "{}", self.0)
        }
    }
}

impl std::error::Error for UnexecutedTickError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl From<anyhow::Error> for UnexecutedTickError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}
