use std::{collections::HashMap, sync::Arc};

use crate::{
    context::{Context, ProjectContext},
    lsp::format_marked_string,
};
use anyhow::Result;
use fuzzt::get_top_n;
use lsp_types::HoverContents;
use mcp_core::{
    tools::ToolHandlerFn,
    types::{CallToolRequest, CallToolResponse, Tool, ToolResponseContent},
};
use serde_json::json;

use super::{
    McpNotification,
    utils::{RequestExtension, error_response, get_info_from_request},
};

pub struct SymbolResolve;

impl SymbolResolve {
    pub fn tool() -> Tool {
        Tool {
            name: "symbol_docs".to_string(),
            description: Some("Resolve a symbol based on its name. Provide any symbol from the file and it will try to resolve it and return documentation about it.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "The name of the symbol to get the documentation for"
                    },
                    "file": {
                        "type": "string",
                        "description": "The absolute path to the file containing the symbol"
                    }
                },
                "required": [ "symbol", "file"]
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
    relative_file: &str,
    request: &CallToolRequest,
) -> Result<CallToolResponse, CallToolResponse> {
    let symbol = request.get_symbol()?;

    let symbols = match project.lsp.document_symbols(relative_file).await {
        Ok(Some(symbols)) => symbols,
        Ok(None) => return Err(error_response("No symbols found")),
        Err(e) => return Err(error_response(&e.to_string())),
    };

    let mut symbol_map = HashMap::new();

    for file_symbol in symbols {
        symbol_map.insert(file_symbol.name.clone(), file_symbol);
    }

    let keys = symbol_map.keys().map(|s| s.as_str()).collect::<Vec<_>>();

    let matches = get_top_n(&symbol, &keys, None, Some(1), None, None);
    let Some(best_match) = matches.first() else {
        return Err(error_response("No match for symbol found"));
    };

    let match_str = best_match.to_string();
    let Some(symbol_match) = symbol_map.get(&match_str) else {
        return Err(error_response("No match for symbol found"));
    };

    let position = symbol_match.location.range.start;

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
