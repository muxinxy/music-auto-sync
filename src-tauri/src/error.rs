use serde::{Deserialize, Serialize};

/// 面向用户的可翻译消息：只包含稳定错误码和参数，文案由前端按语言翻译。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    pub code: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
}

impl UiMessage {
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            params: vec![],
        }
    }

    pub fn with_params(code: impl Into<String>, params: Vec<impl Into<String>>) -> Self {
        Self {
            code: code.into(),
            params: params.into_iter().map(Into::into).collect(),
        }
    }

    pub fn unknown(detail: impl std::fmt::Display) -> Self {
        Self::with_params("unknown", vec![detail.to_string()])
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"code":"unknown"}"#.into())
    }
}

impl std::fmt::Display for UiMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.code)
    }
}

impl std::error::Error for UiMessage {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_params() {
        let message = UiMessage::with_params("file_exists", vec!["D:\\a.mp3"]);
        let encoded = serde_json::to_string(&message).unwrap();
        assert!(encoded.contains("\"code\":\"file_exists\""));
        assert!(encoded.contains("\"params\""));
    }

    #[test]
    fn omits_empty_params() {
        let encoded = serde_json::to_string(&UiMessage::new("no_url")).unwrap();
        assert!(!encoded.contains("\"params\""));
    }
}
