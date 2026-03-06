use anyhow::Result;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

use super::super::BitbucketSource;
use super::super::shared::{apply_auth, parse_bitbucket_json, send};
use super::model::BitbucketDcPage;

impl BitbucketSource {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn datacenter_get_pages_stream<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let per_page = per_page.clamp(1, 100);
        let mut emitted = 0usize;
        let mut start = 0u32;

        loop {
            debug!(url = %url, start, per_page, "fetching Bitbucket Data Center page");
            let mut query_params = params.to_vec();
            query_params.push(("start".to_string(), start.to_string()));
            query_params.push(("limit".to_string(), per_page.to_string()));

            let request = apply_auth(self.client.get(url), token)
                .header("Accept", "application/json")
                .query(&query_params);
            let response = send(request, "page fetch")?;
            let page: BitbucketDcPage<T> = parse_bitbucket_json(response, token, "page fetch")?;
            trace!(
                count = page.values.len(),
                is_last_page = page.is_last_page,
                "decoded Bitbucket Data Center page"
            );

            for item in page.values {
                emit(item)?;
                emitted += 1;
            }

            if page.is_last_page {
                break;
            }

            match page.next_page_start {
                Some(next) => start = next,
                None => break,
            }
        }

        Ok(emitted)
    }

    pub(super) fn datacenter_get_one<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let request = apply_auth(self.client.get(url), token).header("Accept", "application/json");
        let response = send(request, operation)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let item = parse_bitbucket_json(response, token, operation)?;
        Ok(Some(item))
    }
}
