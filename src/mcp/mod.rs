mod crate_docs;
mod symbol_docs;
mod symbol_impl;
mod symbol_references;
mod symbol_resolve;
mod utils;

use std::path::PathBuf;

use crate::context::Context;
use crate::project::TransportType;
use anyhow::Result;
use mcp_core::{
    server::Server,
    transport::{ServerSseTransport, ServerStdioTransport},
    types::{CallToolRequest, CallToolResponse, ServerCapabilities},
};
use serde_json::json;

#[derive(Debug, Clone)]
pub(super) enum McpNotification {
    Request {
        content: CallToolRequest,
        project: PathBuf,
    },
    Response {
        content: CallToolResponse,
        project: PathBuf,
    },
}

pub async fn run_server(context: Context) -> Result<()> {
    let server_protocol = Server::builder("cursor-rust-tools".to_string(), "1.0".to_string())
        .capabilities(ServerCapabilities {
            tools: Some(json!({
                "listChanged": false,
            })),
            ..Default::default()
        })
        .register_tool(
            symbol_docs::SymbolDocs::tool(),
            symbol_docs::SymbolDocs::call(context.clone()),
        )
        .register_tool(
            symbol_impl::SymbolImpl::tool(),
            symbol_impl::SymbolImpl::call(context.clone()),
        )
        .register_tool(
            symbol_references::SymbolReferences::tool(),
            symbol_references::SymbolReferences::call(context.clone()),
        )
        .register_tool(
            symbol_resolve::SymbolResolve::tool(),
            symbol_resolve::SymbolResolve::call(context.clone()),
        )
        .register_tool(
            crate_docs::CrateDocs::tool(),
            crate_docs::CrateDocs::call(context.clone()),
        )
        .build();

    match context.transport() {
        TransportType::Stdio => {
            let transport = ServerStdioTransport::new(server_protocol);
            Server::start(transport).await
        }
        TransportType::Sse { host, port } => {
            let transport = ServerSseTransport::new(host.to_string(), *port, server_protocol);
            Server::start(transport).await
        }
    }
}
