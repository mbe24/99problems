use anyhow::Result;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tracing::{Instrument, debug, debug_span, trace};

use super::super::BitbucketSource;
use super::super::shared::{apply_auth, decode_bitbucket_json, execute_request};
use super::model::BitbucketDcPage;

impl BitbucketSource {
    pub(super) async fn datacenter_get_pages<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let per_page = per_page.clamp(1, 100);
        let mut out = Vec::new();
        let mut start = 0u32;

        loop {
            let page_span = debug_span!("bitbucket.datacenter.page.fetch", start, per_page);
            page_span.in_scope(|| {
                debug!(url = %url, start, per_page, "fetching Bitbucket Data Center page");
            });
            let mut query_params = params.to_vec();
            query_params.push(("start".to_string(), start.to_string()));
            query_params.push(("limit".to_string(), per_page.to_string()));

            let request = apply_auth(self.client.get(url), token)
                .header("Accept", "application/json")
                .query(&query_params);
            let payload = execute_request(request, "page fetch")
                .instrument(page_span.clone())
                .await?;
            let page: BitbucketDcPage<T> = page_span.in_scope(|| {
                debug_span!("bitbucket.datacenter.page.decode", operation = "page fetch")
                    .in_scope(|| decode_bitbucket_json(&payload, token, "page fetch"))
            })?;
            page_span.in_scope(|| {
                trace!(
                    count = page.values.len(),
                    is_last_page = page.is_last_page,
                    "decoded Bitbucket Data Center page"
                );
            });

            out.extend(page.values);

            if page.is_last_page {
                break;
            }

            match page.next_page_start {
                Some(next) => start = next,
                None => break,
            }
        }

        Ok(out)
    }

    pub(super) async fn datacenter_get_one<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let span = debug_span!("bitbucket.datacenter.single.fetch", operation = operation);
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
}
