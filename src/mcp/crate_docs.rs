use std::sync::Arc;

use crate::context::{Context, ProjectContext};
use anyhow::Result;
use mcp_core::{
    tools::ToolHandlerFn,
    types::{CallToolRequest, CallToolResponse, Tool, ToolResponseContent},
};
use serde_json::json;

use super::{
    McpNotification,
    utils::{error_response, get_info_from_request},
};

pub struct CrateDocs;

impl CrateDocs {
    pub fn tool() -> Tool {
        Tool {
            name: "symbol_docs".to_string(),
            description: Some("Get the documentation for a cargo dependency".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dependency": {
                        "type": "string",
                        "description": "The name of the cargo dependency to get the documentation for"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "The optional name of a symbol in the documentation. If not provided, the main readme for the dependency will be returned."
                    },
                },
                "required": ["dependency"]
            }),
        }
    }

    pub fn call(context: Context) -> ToolHandlerFn {
        Box::new(move |request: CallToolRequest| {
            let clone = context.clone();
            Box::pin(async move {
                let (project, relative_file, absolute_file) =
                    match get_info_from_request(&clone, &request).await {
                        Ok(info) => info,
                        Err(response) => return response,
                    };
                if let Err(e) = clone
                    .send_mcp_notification(McpNotification::Request {
                        content: request.clone(),
                        project: absolute_file.clone(),
                    })
                    .await
                {
                    tracing::error!("Failed to send MCP notification: {}", e);
                }
                let response = match handle_request(project, &relative_file, &request).await {
                    Ok(response) => response,
                    Err(response) => response,
                };
                if let Err(e) = clone
                    .send_mcp_notification(McpNotification::Response {
                        content: response.clone(),
                        project: absolute_file.clone(),
                    })
                    .await
                {
                    tracing::error!("Failed to send MCP notification: {}", e);
                }
                response
            })
        })
    }
}

async fn handle_request(
    project: Arc<ProjectContext>,
    _relative_file: &str,
    request: &CallToolRequest,
) -> Result<CallToolResponse, CallToolResponse> {
    let dependency = request
        .arguments
        .as_ref()
        .and_then(|args| args.get("dependency"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| error_response("Dependency is required"))
        .map(|s| s.to_string())?;

    let symbol = request
        .arguments
        .as_ref()
        .and_then(|args| args.get("symbol"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(symbol) = symbol {
        let docs = project
            .docs
            .crate_symbol_docs(&dependency, &symbol)
            .await
            .map_err(|e| error_response(&format!("{e:?}")))?;
        let docs = docs.into_iter().map(|(k, v)| format!("{k}: {v}")).collect();
        Ok(CallToolResponse {
            content: vec![ToolResponseContent::Text { text: docs }],
            is_error: None,
            meta: None,
        })
    } else {
        let docs = project
            .docs
            .crate_docs(&dependency)
            .await
            .map_err(|e| error_response(&format!("{e:?}")))?;
        Ok(CallToolResponse {
            content: vec![ToolResponseContent::Text { text: docs }],
            is_error: None,
            meta: None,
        })
    }
}
