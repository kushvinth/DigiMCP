//! MCP Server state and protocol handler.
//!
//! Human architecture notes:
//! In the Model Context Protocol (MCP) standard, clients (Claude Code, Cursor, Codex)
//! communicate with servers via JSON-RPC 2.0 over standard input and standard output.
//!
//! Why Rust over Node/TypeScript for this:
//!   * **Cold Start Latency**: Modern coding agents frequently spin up subagents and
//!     short-lived background tasks. A Node.js TypeScript MCP server typically takes
//!     500–800 ms cold to initialize the V8 engine, load `node_modules`, and answer the
//!     first `initialize` handshake. DigiMCP compiles into a single ~6 MB static binary
//!     that answers `initialize` in ~5 ms.
//!   * **Graceful Degradation**: If a student hasn't logged in yet, dying on boot
//!     causes the parent agent to throw a fatal transport error and disable the server.
//!     Instead, `Quad::new` boots into a stub state: `tools/list` still succeeds,
//!     and invoking any tool returns a polite, actionable prose error:
//!     `"Not connected yet. Run digi login --portal <host> (or set DIGIMCP_TOKEN)"`.
//!     The agent can then guide the user through authentication interactively.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler};
use serde_json::Value;

use crate::config::Config;
use crate::error::DigiError;
use crate::portal::Portal;

/// Shared server state: one HTTP client per process, cheap to clone.
#[derive(Clone)]
pub struct Quad {
    pub portal: Arc<Portal>,
    router: ToolRouter<Quad>,
}

impl Quad {
    /// Build the server. A missing or invalid configuration is not fatal: the
    /// server still starts, and every tool explains the fix on first use, so
    /// the agent can walk the student through login instead of dying at boot.
    pub fn new(cfg: Result<Config, DigiError>) -> Self {
        let portal = match cfg.and_then(Portal::new) {
            Ok(portal) => portal,
            Err(err) => {
                eprintln!("digimcp: {err}");
                let stub = Config {
                    token: String::new(),
                    base_url: String::new(),
                    timezone: crate::config::DEFAULT_TIMEZONE.into(),
                    timeout_ms: crate::config::DEFAULT_TIMEOUT_MS,
                    ukid: None,
                    expires_at: None,
                    college: None,
                };
                Portal::new(stub).expect("stub client")
            }
        };
        Self {
            portal: Arc::new(portal),
            router: crate::registry::router(),
        }
    }

    /// True when the token is present and not expired.
    pub fn ready(&self) -> Result<&Portal, DigiError> {
        let cfg = self.portal.config();
        if cfg.token.is_empty() {
            return Err(DigiError::Config(
                "Not connected yet. Run `digi login --portal <institute>.digiicampus.com` \
                 (or set DIGIMCP_TOKEN) and retry."
                    .into(),
            ));
        }
        if cfg.is_expired() {
            return Err(DigiError::Unauthorized);
        }
        Ok(&self.portal)
    }
}

impl ServerHandler for Quad {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("digimcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "DigiMCP interfaces with a student's DigiiCampus portal: classes, deadlines, assignments, \
                 course materials, attendance, quizzes, feedback forms, and grades. Use `due` for workload or deadlines \
                 (it merges quizzes and assignments), `materials` and `material` for lecture notes and slides, \
                 `assignment` for one item by name, `attendance` for percentages and bunks, `grades` for results, \
                 `quiz_questions` to inspect questions before answering, `feedback_forms` for pending surveys, and \
                 `submit_assignment`, `submit_quiz`, `fill_feedback`, or `fill_form` to execute actions. \
                 Pass a class name wherever a class is asked for. All write actions support `dry_run: true` to preview \
                 and verify compliance before submitting.",
            )
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::CallToolResponse, ErrorData>>
    + rmcp::service::MaybeSendFuture
    + '_ {
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.router.call(call)
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListToolsResult, ErrorData>>
    + rmcp::service::MaybeSendFuture
    + '_ {
        std::future::ready(Ok(rmcp::model::ListToolsResult {
            tools: self.router.list_all(),
            ..Default::default()
        }))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.router.get(name).cloned()
    }
}

/// A tool call's success path: RTK-compacted JSON the agent can read directly.
pub fn ok(value: Value) -> Result<CallToolResult, ErrorData> {
    let pruned = crate::simplify::prune(value);
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string(&pruned).unwrap_or_else(|_| "{}".into()),
    )]))
}

/// A tool call's failure path: an actionable message, never a stack trace.
pub fn fail(err: DigiError) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        err.to_string(),
    )]))
}

/// Non-generic completion path so all 16 tools share one serialization & error-mapping implementation.
pub fn finish_result(res: Result<Value, DigiError>) -> Result<CallToolResult, ErrorData> {
    match res {
        Ok(value) => ok(value),
        Err(err) => fail(err),
    }
}

/// Run a tool implementation, mapping errors onto readable results.
#[inline(always)]
pub async fn finish<F>(future: F) -> Result<CallToolResult, ErrorData>
where
    F: std::future::Future<Output = Result<Value, DigiError>>,
{
    finish_result(future.await)
}
