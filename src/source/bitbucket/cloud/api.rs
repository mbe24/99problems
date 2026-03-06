use anyhow::Result;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

use super::super::shared::{apply_auth, parse_bitbucket_json, send};
use super::super::{BitbucketSource, PAGE_SIZE};
use super::model::BitbucketPage;

impl BitbucketSource {
    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) fn cloud_get_one<T: DeserializeOwned>(
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cloud_get_pages_stream<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let per_page = Self::bounded_per_page(per_page);
        let mut emitted = 0usize;
        let mut next_url = Some(url.to_string());
        let mut first = true;

        while let Some(current_url) = next_url {
            debug!(url = %current_url, per_page, "fetching Bitbucket cloud page");
            let mut request = apply_auth(self.client.get(&current_url), token)
                .header("Accept", "application/json");
            if first {
                let mut merged_params = params.to_vec();
                merged_params.push(("pagelen".to_string(), per_page.to_string()));
                request = request.query(&merged_params);
                first = false;
            }

            let response = send(request, "page fetch")?;
            let page: BitbucketPage<T> = parse_bitbucket_json(response, token, "page fetch")?;
            trace!(count = page.values.len(), "decoded Bitbucket cloud page");
            for item in page.values {
                emit(item)?;
                emitted += 1;
            }
            next_url = page.next;
        }

        Ok(emitted)
    }
}
