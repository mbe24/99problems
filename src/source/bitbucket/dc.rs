use anyhow::Result;

use super::BitbucketSource;
use crate::error::AppError;
use crate::model::Conversation;
use crate::source::FetchRequest;

impl BitbucketSource {
    pub(super) fn fetch_dc_stream(
        &self,
        _req: &FetchRequest,
        _emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        Err(AppError::provider(format!(
            "Bitbucket deployment 'selfhosted' is not implemented yet for base URL '{}'. Use --deployment cloud for now.",
            self.base_url
        ))
        .with_provider("bitbucket")
        .into())
    }
}
