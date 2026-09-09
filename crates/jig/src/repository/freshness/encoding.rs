use sha2::{Digest, Sha256};

use jig_contract::{TargetId, freshness::TARGET_IDENTITY_SCHEMA_VERSION};

/// V1: NUL-terminated domain followed by length-framed fields. Integers are
/// fixed-width big-endian fields; an optional field has its own presence tag.
pub(super) struct IdentityEncoder(Sha256);

impl IdentityEncoder {
    pub(super) fn new(domain: &str, epoch: u32) -> Self {
        let mut hash = Sha256::new();
        hash.update(domain.as_bytes());
        hash.update([0]);
        let mut encoder = Self(hash);
        encoder.field(&epoch.to_be_bytes());
        encoder.field(&TARGET_IDENTITY_SCHEMA_VERSION.to_be_bytes());
        encoder
    }

    pub(super) fn field(&mut self, value: &[u8]) {
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    pub(super) fn text(&mut self, value: &str) {
        self.field(value.as_bytes());
    }

    pub(super) fn number(&mut self, value: u64) {
        self.field(&value.to_be_bytes());
    }

    pub(super) fn optional(&mut self, value: Option<&str>) {
        self.field(&[u8::from(value.is_some())]);
        if let Some(value) = value {
            self.text(value);
        }
    }

    pub(super) fn target(&mut self, target: &TargetId) {
        self.text(target.component.as_str());
        self.text(target.action.as_str());
    }

    pub(super) fn finish(self) -> String {
        format!("sha256:{:x}", self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn externally_computed_v1_vectors_pin_byte_encoding() {
        // Independently computed with Python struct.pack('>Q', len(field))
        // and hashlib.sha256, as documented in target-freshness-benchmark.md.
        let mut source = IdentityEncoder::new("jig-target-source-v1", 9);
        source.text("exhaustive");
        source.number(1);
        source.text("apps/web/**");
        source.number(0);
        source.number(0);
        assert_eq!(
            source.finish(),
            "sha256:f69025da6d1624321cbe4635e1a2e247e03f7ded42e4bbbd88d19cb72e91f089"
        );
        let mut identity = IdentityEncoder::new("jig-target-identity-v1", 9);
        identity.target(&"web:test".parse().unwrap());
        identity.text("source");
        identity.text("authority");
        identity.text("dependency");
        assert_eq!(
            identity.finish(),
            "sha256:1015e16914c1f65117d6da50fe7101e86728939559164cda8021c6378985416c"
        );
    }

    #[test]
    fn framing_distinguishes_field_boundaries_absence_and_domains() {
        let token = |domain: &str, fields: &[Option<&str>]| {
            let mut hash = IdentityEncoder::new(domain, 9);
            for field in fields {
                hash.optional(*field);
            }
            hash.finish()
        };
        assert_ne!(
            token("example", &[Some("ab"), Some("c")]),
            token("example", &[Some("a"), Some("bc")])
        );
        assert_ne!(token("example", &[None]), token("example", &[Some("")]));
        assert_ne!(
            token("example", &[Some("a")]),
            token("different", &[Some("a")])
        );
    }
}
