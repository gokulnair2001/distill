//! `distill-mcp` — run distill as an MCP server over stdio.
//!
//! Built only with `--features mcp`. Point an MCP client (e.g. Claude Code) at
//! this binary; it speaks the Model Context Protocol on stdin/stdout, so it
//! writes no logging to stdout.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    distill::mcp::run().await
}
