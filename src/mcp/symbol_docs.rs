use std::sync::Arc;

use crate::{
    context::{Context, ProjectContext},
    lsp::format_marked_string,
};
use anyhow::Result;
use lsp_types::HoverContents;
use mcp_core::{
    tools::ToolHandlerFn,
    types::{CallToolRequest, CallToolResponse, Tool, ToolResponseContent},
};
use serde_json::json;

use super::{
    McpNotification,
    utils::{
        RequestExtension, error_response, find_symbol_position_in_file, get_info_from_request,
    },
};

pub struct SymbolDocs;

impl SymbolDocs {
    pub fn tool() -> Tool {
        Tool {
            name: "symbol_docs".to_string(),
            description: Some("Get the documentation for a symbol".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "line": {
                        "type": "number",
                        "description": "The line number of the symbol in the file (1 based)"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "The name of the symbol to get the documentation for"
                    },
                    "file": {
                        "type": "string",
                        "description": "The absolute path to the file containing the symbol"
                    }
                },
                "required": ["line", "symbol", "file"]
            }),
        }
    }

    pub fn call(context: Context) -> ToolHandlerFn {
        Box::new(move |request: CallToolRequest| {
            let clone = context.clone();
            Box::pin(async move {
                let (project, relative_file, absolute_file) =
                    match get_info_from_request(&clone, &request) {
                        Ok(info) => info,
                        Err(response) => return response,
                    };
                clone.send_mcp_notification(McpNotification::Request {
                    content: request.clone(),
                    project: absolute_file.clone(),
                });
                let response = match handle_request(project, &relative_file, &request).await {
                    Ok(response) => response,
                    Err(response) => response,
                };
                clone.send_mcp_notification(McpNotification::Response {
                    content: response.clone(),
                    project: absolute_file.clone(),
                });
                response
            })
        })
    }
}

async fn handle_request(
    project: Arc<ProjectContext>,
    relative_file: &str,
    request: &CallToolRequest,
) -> Result<CallToolResponse, CallToolResponse> {
    let line = request.get_line()?;
    let symbol = request.get_symbol()?;

    let position = find_symbol_position_in_file(&project, relative_file, &symbol, line)
        .await
        .map_err(|e| error_response(&e))?;

    let Some(hover) = project
        .lsp
        .hover(relative_file, position)
        .await
        .map_err(|e| error_response(&e.to_string()))?
    else {
        return Err(error_response("No hover information found"));
    };

    let response = match hover.contents {
        HoverContents::Scalar(s) => format_marked_string(&s),
        HoverContents::Array(a) => a
            .into_iter()
            .map(|s| format_marked_string(&s))
            .collect::<Vec<_>>()
            .join("\n"),
        HoverContents::Markup(m) => m.value,
    };

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text { text: response }],
        is_error: None,
        meta: None,
    })
}
