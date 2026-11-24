use crate::types::{ExecuteResponse, QueryContext, ResultItem};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const METHOD_HANDSHAKE: &str = "tomoyo/handshake";
pub const METHOD_QUERY: &str = "tomoyo/query";
pub const METHOD_EXECUTE: &str = "tomoyo/execute";
pub const METHOD_EVENT: &str = "tomoyo/event";
pub const METHOD_CALL: &str = "tomoyo/call";
pub const METHOD_SHUTDOWN: &str = "tomoyo/shutdown";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeParams {
    pub protocol_version: u32,
    pub host_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    pub protocol_version: u32,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub items: Vec<ResultItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub action_id: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub payload: Value,
    /// Protocol v3: merged plugin settings injected by `PluginHost`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_settings: BTreeMap<String, Value>,
    /// Protocol v3: decrypted secrets declared by this plugin (OS Keychain / DPAPI).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_secrets: BTreeMap<String, String>,
}

impl ExecuteRequest {
    pub fn new(action_id: impl Into<String>, item_id: impl Into<String>, payload: Value) -> Self {
        Self {
            action_id: action_id.into(),
            item_id: item_id.into(),
            payload,
            plugin_settings: BTreeMap::new(),
            plugin_secrets: BTreeMap::new(),
        }
    }
}

pub type QueryRequest = QueryContext;
pub type ExecuteResult = ExecuteResponse;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_rpc_request_response() {
        let req = JsonRpcRequest::new(1, METHOD_QUERY, serde_json::json!({"raw": "hi"}));
        let line = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.method, METHOD_QUERY);

        let resp = JsonRpcResponse::ok(1, serde_json::json!({"items": []}));
        let line = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&line).unwrap();
        assert!(parsed.error.is_none());
        assert!(parsed.result.is_some());
    }
}
