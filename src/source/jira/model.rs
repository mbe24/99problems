use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub(super) struct JiraSearchResponse {
    #[serde(rename = "startAt")]
    pub(super) start_at: Option<u32>,
    #[serde(rename = "maxResults")]
    pub(super) max_results: Option<u32>,
    pub(super) total: Option<u32>,
    #[serde(rename = "isLast")]
    pub(super) is_last: Option<bool>,
    #[serde(rename = "nextPageToken")]
    pub(super) next_page_token: Option<String>,
    pub(super) issues: Vec<JiraIssueItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraIssueItem {
    pub(super) key: String,
    pub(super) fields: JiraIssueFields,
}

#[derive(Deserialize)]
pub(super) struct JiraIssueFields {
    pub(super) summary: String,
    pub(super) description: Option<Value>,
    pub(super) status: JiraStatus,
}

#[derive(Deserialize)]
pub(super) struct JiraStatus {
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct JiraCommentsPage {
    #[serde(rename = "startAt")]
    pub(super) start_at: u32,
    #[serde(rename = "maxResults")]
    pub(super) max_results: u32,
    pub(super) total: u32,
    pub(super) comments: Vec<JiraCommentItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraCommentItem {
    pub(super) author: Option<JiraAuthor>,
    pub(super) created: String,
    pub(super) body: Value,
}

#[derive(Deserialize)]
pub(super) struct JiraAuthor {
    #[serde(rename = "displayName")]
    pub(super) display_name: String,
}

pub(super) fn extract_adf_text(value: &Value) -> String {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if let Some(Value::String(text)) = map.get("text") {
                    out.push(text.clone());
                }
                if let Some(content) = map.get("content") {
                    walk(content, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }

    let mut chunks = Vec::new();
    walk(value, &mut chunks);
    chunks.join(" ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_adf_text_reads_nested_nodes() {
        let value: Value = serde_json::json!({
            "type": "doc",
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "world"}]}
            ]
        });
        assert_eq!(extract_adf_text(&value), "Hello world");
    }
}
