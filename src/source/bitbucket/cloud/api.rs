use anyhow::Result;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tracing::{Instrument, debug, debug_span, trace};

use super::super::shared::{apply_auth, decode_bitbucket_json, execute_request};
use super::super::{BitbucketSource, PAGE_SIZE};
use super::model::BitbucketPage;

impl BitbucketSource {
    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) async fn cloud_get_one<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let span = debug_span!("bitbucket.cloud.single.fetch", operation = operation);
        async {
            let request =
                apply_auth(self.client.get(url), token).header("Accept", "application/json");
            let payload = execute_request(request, operation)
                .instrument(span.clone())
                .await?;
            if payload.status == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let item = decode_bitbucket_json(&payload, token, operation)?;
            Ok(Some(item))
        }
        .instrument(span.clone())
        .await
    }

    pub(super) async fn cloud_get_pages<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let per_page = Self::bounded_per_page(per_page);
        let mut out = Vec::new();
        let mut next_url = Some(url.to_string());
        let mut first = true;

        while let Some(current_url) = next_url {
            let page_span = debug_span!("bitbucket.cloud.page.fetch", per_page);
            page_span
                .in_scope(|| debug!(url = %current_url, per_page, "fetching Bitbucket cloud page"));
            let mut request = apply_auth(self.client.get(&current_url), token)
                .header("Accept", "application/json");
            if first {
                let mut merged_params = params.to_vec();
                merged_params.push(("pagelen".to_string(), per_page.to_string()));
                request = request.query(&merged_params);
                first = false;
            }

            let payload = execute_request(request, "page fetch")
                .instrument(page_span.clone())
                .await?;
            let page: BitbucketPage<T> = page_span.in_scope(|| {
                debug_span!("bitbucket.cloud.page.decode", operation = "page fetch")
                    .in_scope(|| decode_bitbucket_json(&payload, token, "page fetch"))
            })?;
            page_span
                .in_scope(|| trace!(count = page.values.len(), "decoded Bitbucket cloud page"));
            out.extend(page.values);
            next_url = page.next;
        }

        Ok(out)
    }
}
