mod symbol_docs;
mod symbol_impl;
mod symbol_references;
mod symbol_resolve;
mod utils;

use crate::context::Context;
use crate::project::TransportType;
use anyhow::Result;
use mcp_core::{
    server::Server,
    transport::{ServerSseTransport, ServerStdioTransport},
    types::ServerCapabilities,
};
use serde_json::json;
use symbol_docs::SymbolDocs;

pub async fn run_server(context: Context) -> Result<()> {
    let server_protocol = Server::builder("echo".to_string(), "1.0".to_string())
        .capabilities(ServerCapabilities {
            tools: Some(json!({
                "listChanged": false,
            })),
            ..Default::default()
        })
        .register_tool(SymbolDocs::tool(), SymbolDocs::call(context.clone()))
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
