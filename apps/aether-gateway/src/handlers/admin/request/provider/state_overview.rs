use super::*;
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;

impl<'a> AdminAppState<'a> {
    pub(crate) async fn read_codex_turn_state_overview(
        &self,
        page: usize,
        page_size: usize,
        search: &str,
    ) -> Result<Value, GatewayError> {
        let providers = self
            .list_provider_catalog_providers(false)
            .await?
            .into_iter()
            .filter(|provider| provider.provider_type.eq_ignore_ascii_case("codex"))
            .map(|provider| (provider.id.clone(), provider))
            .collect::<BTreeMap<_, _>>();
        let provider_ids = providers.keys().cloned().collect::<Vec<_>>();
        let mut keys = if provider_ids.is_empty() {
            Vec::new()
        } else {
            self.list_provider_catalog_key_summaries_by_provider_ids(&provider_ids)
                .await?
        };
        let search = search.trim().to_lowercase();
        keys.retain(|key| {
            key.auth_type.eq_ignore_ascii_case("oauth")
                && providers.get(&key.provider_id).is_some_and(|provider| {
                    search.is_empty()
                        || format!("{} {}", provider.name, key.name)
                            .to_lowercase()
                            .contains(&search)
                })
        });
        keys.sort_by(|a, b| {
            (&a.provider_id, &a.name, &a.id).cmp(&(&b.provider_id, &b.name, &b.id))
        });
        let total = keys.len();
        let page_size = page_size.clamp(1, 20);
        let page = page.clamp(1, total.div_ceil(page_size).max(1));
        let accounts = stream::iter(
            keys.into_iter()
                .skip((page - 1) * page_size)
                .take(page_size),
        )
        .map(|key| {
            let provider = &providers[&key.provider_id];
            async move {
                let diagnostics = tokio::time::timeout(
                    Duration::from_secs(5),
                    crate::execution_runtime::codex_turn_state::diagnostics::read(
                        self.app,
                        &key.provider_id,
                        &key.id,
                    ),
                )
                .await;
                let diagnostics = match diagnostics {
                    Ok(Ok(value)) => Some(value),
                    _ => None,
                };
                json!({
                    "provider_id":key.provider_id, "provider_name":provider.name,
                    "key_id":key.id, "key_name":key.name,
                    "active":provider.is_active && key.is_active,
                    "read_failed":diagnostics.is_none(), "diagnostics":diagnostics,
                })
            }
        })
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
        Ok(json!({"total":total,"page":page,"page_size":page_size,"accounts":accounts}))
    }
}
