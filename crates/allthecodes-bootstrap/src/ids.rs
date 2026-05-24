//! 品牌类型 — 防止裸 String 混用
//!
//! 对应 TypeScript: bootstrap/src/types/ids.ts

use serde::{Deserialize, Serialize};

/// 会话 ID 品牌类型。
///
/// 包装 UUID v4 字符串，在类型层面区分于其他 `String`。
/// 所有需要 session ID 的函数签名都应使用此类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// 生成一个新的随机会话 ID (UUID v4)。
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// 从已有字符串构造 (用于 session resume 等场景)。
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// 获取内部字符串引用。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_generates_unique_ids() {
        let a = SessionId::new();
        let b = SessionId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn from_string_roundtrip() {
        let id = SessionId::from_string("test-session-123");
        assert_eq!(id.as_str(), "test-session-123");
        assert_eq!(id.to_string(), "test-session-123");
    }

    #[test]
    fn display_matches_inner() {
        let id = SessionId::from_string("abc");
        assert_eq!(format!("{}", id), "abc");
    }

    #[test]
    fn serde_roundtrip() {
        let id = SessionId::from_string("ser-test");
        let json = serde_json::to_string(&id).unwrap();
        let back: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
