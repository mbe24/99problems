use reqwest::blocking::RequestBuilder;

pub(in crate::source::bitbucket) fn apply_auth(
    req: RequestBuilder,
    token: Option<&str>,
) -> RequestBuilder {
    match resolve_auth_mode(token) {
        AuthMode::None => req,
        AuthMode::Bearer(token) => req.bearer_auth(token),
        AuthMode::Basic { user, secret } => req.basic_auth(user, Some(secret)),
    }
}

fn resolve_auth_mode(token: Option<&str>) -> AuthMode<'_> {
    match token {
        Some(t) if t.contains(':') => {
            let (user, secret) = t.split_once(':').unwrap_or_default();
            AuthMode::Basic { user, secret }
        }
        Some(t) => AuthMode::Bearer(t),
        None => AuthMode::None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AuthMode<'a> {
    None,
    Bearer(&'a str),
    Basic { user: &'a str, secret: &'a str },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_auth_mode_prefers_explicit_basic() {
        assert_eq!(
            resolve_auth_mode(Some("user:pass")),
            AuthMode::Basic {
                user: "user",
                secret: "pass"
            }
        );
    }

    #[test]
    fn resolve_auth_mode_uses_bearer_for_plain_tokens() {
        assert_eq!(
            resolve_auth_mode(Some("ATxxxx")),
            AuthMode::Bearer("ATxxxx")
        );
    }
}
