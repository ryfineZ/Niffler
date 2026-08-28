mod memory;
mod postgres;
mod types;

pub use memory::InMemoryContentModerationEvidenceRepository;
pub use postgres::SqlxContentModerationEvidenceRepository;
pub use types::{
    ContentModerationEvidenceReadRepository, ContentModerationEvidenceRepository,
    ContentModerationEvidenceWriteRepository, InsertContentModerationEvidenceRecord,
    StoredContentModerationEvidence,
};

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ContentModerationEvidenceReadRepository, ContentModerationEvidenceWriteRepository,
        InMemoryContentModerationEvidenceRepository, InsertContentModerationEvidenceRecord,
    };
    use crate::DataLayerError;

    fn sample_record(id: &str, input_text: Option<&str>) -> InsertContentModerationEvidenceRecord {
        InsertContentModerationEvidenceRecord {
            id: id.to_string(),
            request_id: "req-1".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("api-key-1".to_string()),
            provider_id: Some("provider-1".to_string()),
            upstream_service_id: Some("service-1".to_string()),
            upstream_account_id: Some("account-1".to_string()),
            moderation_model: "omni-moderation-latest".to_string(),
            input_sha256: "sha256-1".to_string(),
            input_text: input_text.map(str::to_string),
            categories: json!({"violence": true}),
            category_scores: json!({"violence": 0.91}),
            flagged: true,
            created_at_unix_secs: 1_000,
            expires_at_unix_secs: 2_000,
        }
    }

    #[tokio::test]
    async fn memory_repository_inserts_and_reads_evidence() -> Result<(), DataLayerError> {
        let repository = InMemoryContentModerationEvidenceRepository::default();

        let stored = repository
            .insert(sample_record("evidence-1", Some("flagged input")))
            .await?;
        let found = repository.find_by_id("evidence-1").await?;

        assert_eq!(stored.id, "evidence-1");
        assert_eq!(
            found.as_ref().and_then(|item| item.input_text.as_deref()),
            Some("flagged input")
        );
        assert_eq!(
            found.as_ref().map(|item| item.input_sha256.as_str()),
            Some("sha256-1")
        );
        assert_eq!(
            found.as_ref().map(|item| &item.categories),
            Some(&json!({"violence": true}))
        );

        Ok(())
    }

    #[tokio::test]
    async fn memory_repository_redacts_expired_input_text_only() -> Result<(), DataLayerError> {
        let repository = InMemoryContentModerationEvidenceRepository::default();
        repository
            .insert(sample_record("evidence-1", Some("flagged input")))
            .await?;

        let redacted = repository.redact_expired_input_text(2_500, 100).await?;
        let found = repository
            .find_by_id("evidence-1")
            .await?
            .expect("evidence should remain after redaction");

        assert_eq!(redacted, 1);
        assert!(found.input_text.is_none());
        assert_eq!(found.input_sha256, "sha256-1");
        assert_eq!(found.categories, json!({"violence": true}));
        assert_eq!(found.redacted_at_unix_secs, Some(2_500));

        Ok(())
    }
}
