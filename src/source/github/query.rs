use super::GITHUB_API_BASE;

/// Extract `owner/repo` from a query string containing `repo:owner/repo`.
#[must_use]
pub fn extract_repo(query: &str) -> Option<String> {
    query
        .split_whitespace()
        .find(|t| t.starts_with("repo:"))
        .map(|t| t.trim_start_matches("repo:").to_string())
}

pub(super) fn repo_from_repository_url(url: &str) -> Option<String> {
    let prefix = format!("{GITHUB_API_BASE}/repos/");
    url.strip_prefix(&prefix)
        .map(std::string::ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_repo_finds_token() {
        assert_eq!(
            extract_repo("is:issue state:closed repo:owner/repo Event"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn extract_repo_returns_none_when_absent() {
        assert_eq!(extract_repo("is:issue state:closed Event"), None);
    }

    #[test]
    fn repo_from_repository_url_parses_repo() {
        assert_eq!(
            repo_from_repository_url("https://api.github.com/repos/owner/repo"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn repo_from_repository_url_returns_none_for_non_github_api_url() {
        assert_eq!(
            repo_from_repository_url("https://example.com/repos/owner/repo"),
            None
        );
    }
}
