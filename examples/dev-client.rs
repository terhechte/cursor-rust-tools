// Not really an example. Instead, just a small client to test the MCP server.

use anyhow::Result;
use mcp_core::{
    client::ClientBuilder,
    transport::ClientSseTransportBuilder,
    types::{ClientCapabilities, Implementation},
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let client = ClientBuilder::new(
        ClientSseTransportBuilder::new("http://localhost:4000/sse".to_string()).build(),
    )
    .build();
    client.open().await?;

    client
        .initialize(
            Implementation {
                name: "echo".to_string(),
                version: "1.0".to_string(),
            },
            ClientCapabilities::default(),
        )
        .await?;

    let response = client
        .call_tool(
            "symbol_references",
            Some(json!({
              "file": "/Users/terhechte/Developer/Rust/supatest/src/main.rs",
              "line": 26,
              "symbol": "ApiKey"
            })),
        )
        .await?;
    dbg!(&response);
    Ok(())
}
