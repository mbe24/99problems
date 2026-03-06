use reqwest::blocking::RequestBuilder;

pub(super) fn apply_auth(
    req: RequestBuilder,
    token: Option<&str>,
    account_email: Option<&str>,
) -> RequestBuilder {
    match resolve_auth_mode(token, account_email) {
        AuthMode::None => req,
        AuthMode::Bearer(token) => req.bearer_auth(token),
        AuthMode::Basic { user, secret } => req.basic_auth(user, Some(secret)),
    }
}

pub(super) fn auth_hint(token: Option<&str>, account_email: Option<&str>) -> &'static str {
    if token.is_some() {
        if account_email.is_some() {
            "Check Bitbucket token credentials and scopes."
        } else {
            "If this is an Atlassian API token, also set account email (--account-email, BITBUCKET_ACCOUNT_EMAIL, or [instances.<alias>].account_email), or pass --token as email:token."
        }
    } else {
        "No Bitbucket token detected. Set --token, BITBUCKET_TOKEN, or [instances.<alias>].token."
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AuthMode<'a> {
    None,
    Bearer(&'a str),
    Basic { user: &'a str, secret: &'a str },
}

fn resolve_auth_mode<'a>(token: Option<&'a str>, account_email: Option<&'a str>) -> AuthMode<'a> {
    match token {
        Some(t) if t.contains(':') => {
            let (user, secret) = t.split_once(':').unwrap_or_default();
            AuthMode::Basic { user, secret }
        }
        Some(t) if looks_like_atlassian_bearer(t) => AuthMode::Bearer(t),
        Some(t) => match account_email {
            Some(email) => AuthMode::Basic {
                user: email,
                secret: t,
            },
            None => AuthMode::Bearer(t),
        },
        None => AuthMode::None,
    }
}

fn looks_like_atlassian_bearer(token: &str) -> bool {
    token.starts_with("AT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_auth_mode_prefers_explicit_basic() {
        assert_eq!(
            resolve_auth_mode(Some("user:pass"), Some("mail@example.com")),
            AuthMode::Basic {
                user: "user",
                secret: "pass"
            }
        );
    }

    #[test]
    fn resolve_auth_mode_uses_bearer_for_atlassian_tokens() {
        assert_eq!(
            resolve_auth_mode(Some("ATxxxx"), Some("mail@example.com")),
            AuthMode::Bearer("ATxxxx")
        );
    }

    #[test]
    fn resolve_auth_mode_uses_email_basic_for_non_atlassian_tokens() {
        assert_eq!(
            resolve_auth_mode(Some("plain-token"), Some("mail@example.com")),
            AuthMode::Basic {
                user: "mail@example.com",
                secret: "plain-token"
            }
        );
    }
}
