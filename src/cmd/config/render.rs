pub(crate) fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return "****".to_string();
    }
    for prefix in ["github_pat_", "glpat_", "ghp_", "AT"] {
        if value.starts_with(prefix) {
            return format!("{prefix}****");
        }
    }
    if value.len() <= 4 {
        return "****".to_string();
    }
    format!("{}****", &value[..4])
}

pub(crate) fn render_value(value: &str, is_secret: bool, show_secrets: bool) -> String {
    if is_secret && !show_secrets {
        mask_secret(value)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_prefix_tokens() {
        assert_eq!(mask_secret("glpat_abc123"), "glpat_****");
        assert_eq!(mask_secret("ATATT3xFfGF05..."), "AT****");
    }
}
