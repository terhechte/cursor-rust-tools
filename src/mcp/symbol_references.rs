use std::sync::Arc;

use crate::context::{Context, ProjectContext};
use anyhow::Result;
use mcp_core::{
    tools::ToolHandlerFn,
    types::{CallToolRequest, CallToolResponse, Tool, ToolResponseContent},
};
use serde_json::json;

use super::utils::{
    RequestExtension, error_response, find_symbol_position_in_file, get_file_lines,
    get_info_from_request,
};

pub struct SymbolReferences;

impl SymbolReferences {
    pub fn tool() -> Tool {
        Tool {
            name: "symbol_references".to_string(),
            description: Some("Get all the references for a symbol. Will return a list of files that contain the symbol including a preview of the usage.".to_string()),
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
                let (project, relative_file, _) = match get_info_from_request(&clone, &request) {
                    Ok(info) => info,
                    Err(response) => return response,
                };
                match handle_request(project, &relative_file, &request).await {
                    Ok(response) => response,
                    Err(response) => response,
                }
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

    let Some(references) = project
        .lsp
        .find_references(relative_file, position)
        .await
        .map_err(|e| error_response(&e.to_string()))?
    else {
        return Err(error_response("No references found"));
    };

    let mut contents = String::new();
    for reference in references {
        let Ok(Some(lines)) = get_file_lines(
            reference.uri.path(),
            reference.range.start.line,
            reference.range.end.line,
            4,
            4,
        ) else {
            continue;
        };
        contents.push_str(&format!("## {}\n```\n{}\n```\n", reference.uri, lines));
    }

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text { text: contents }],
        is_error: None,
        meta: None,
    })
}
