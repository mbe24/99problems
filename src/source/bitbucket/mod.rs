use anyhow::Result;
use async_trait::async_trait;
use reqwest::blocking::Client;
use tokio::task::block_in_place;
use tracing::debug_span;

use super::{FetchRequest, Source};
use crate::error::AppError;
use crate::model::Conversation;

mod cloud;
mod datacenter;
mod query;
mod shared;

const BITBUCKET_CLOUD_API_BASE: &str = "https://api.bitbucket.org/2.0";
const PAGE_SIZE: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BitbucketDeployment {
    Cloud,
    Selfhosted,
}

impl BitbucketDeployment {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "cloud" => Ok(Self::Cloud),
            "selfhosted" => Ok(Self::Selfhosted),
            other => Err(AppError::usage(format!(
                "Invalid bitbucket deployment '{other}'. Supported: cloud, selfhosted."
            ))
            .into()),
        }
    }
}

pub struct BitbucketSource {
    pub(super) client: Client,
    deployment: BitbucketDeployment,
    pub(super) base_url: String,
}

impl BitbucketSource {
    /// Create a Bitbucket source client.
    ///
    /// # Errors
    ///
    /// Returns an error if deployment is missing/invalid or the HTTP client
    /// cannot be constructed.
    pub fn new(platform_url: Option<String>, deployment: Option<String>) -> Result<Self> {
        let deployment = deployment.ok_or_else(|| {
            AppError::usage(
                "Bitbucket deployment is required. Set [instances.<alias>].deployment or pass --deployment (cloud|selfhosted).",
            )
        })?;
        let deployment = BitbucketDeployment::parse(&deployment)?;
        let base_url = match deployment {
            BitbucketDeployment::Cloud => BITBUCKET_CLOUD_API_BASE.to_string(),
            BitbucketDeployment::Selfhosted => platform_url
                .ok_or_else(|| {
                    AppError::usage(
                        "Bitbucket selfhosted deployment requires --url or [instances.<alias>].url.",
                    )
                })?
                .trim_end_matches('/')
                .to_string(),
        };

        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            client,
            deployment,
            base_url,
        })
    }
}

#[async_trait(?Send)]
impl Source for BitbucketSource {
    async fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        block_in_place(|| {
            let _span = debug_span!("bitbucket.fetch_stream").entered();
            match self.deployment {
                BitbucketDeployment::Cloud => self.fetch_cloud_stream(req, emit),
                BitbucketDeployment::Selfhosted => self.fetch_datacenter_stream(req, emit),
            }
        })
    }
}
