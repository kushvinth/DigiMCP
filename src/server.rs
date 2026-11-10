//! Tool declarations.
//!
//! The registry is assembled by hand from free functions instead of the
//! `#[tool_router]` macro: the macro is global, and one bad attribute inside it
//! silently drops every tool from `tools/list`, which is invisible to unit
//! tests and infuriating in production. Explicit wiring fails loudly and can be
//! asserted in a unit test (`tool_names()`), which the e2e suite does.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler};
use serde_json::Value;

use crate::config::Config;
use crate::error::QuadError;
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
    pub fn new(cfg: Result<Config, QuadError>) -> Self {
        let portal = match cfg.and_then(Portal::new) {
            Ok(portal) => portal,
            Err(err) => {
                eprintln!("quad: {err}");
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
    pub fn ready(&self) -> Result<&Portal, QuadError> {
        let cfg = self.portal.config();
        if cfg.token.is_empty() {
            return Err(QuadError::Config(
                "Not connected yet. Run `quad login --portal <institute>.digiicampus.com` \
                 (or set QUAD_TOKEN) and retry."
                    .into(),
            ));
        }
        if cfg.is_expired() {
            return Err(QuadError::Unauthorized);
        }
        Ok(&self.portal)
    }
}

impl ServerHandler for Quad {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            // The crate is `quad-mcp`; agents should see the command they run.
            .with_server_info(Implementation::new("quad", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "quad reads a student's Digiicampus portal: classes, deadlines, assignments, \
                 attendance and grades. Use `due` for anything about workload or what is next \
                 (it merges quizzes and assignments), `assignment` for one item by name, \
                 `attendance` for percentages and how many classes can still be missed, and \
                 `grades` for results. Pass a class *name* (e.g. \"Physics\") wherever a class \
                 is asked for; quad resolves ids itself. Times are RFC 3339 in the college \
                 timezone. Read-only: nothing is ever submitted.",
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

/// A tool call's success path: pretty JSON the agent can read directly.
pub fn ok(value: Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into()),
    )]))
}

/// A tool call's failure path: an actionable message, never a stack trace.
pub fn fail(err: QuadError) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        err.to_string(),
    )]))
}

/// Run a tool implementation, mapping errors onto readable results.
pub async fn finish<F>(future: F) -> Result<CallToolResult, ErrorData>
where
    F: std::future::Future<Output = Result<Value, QuadError>>,
{
    match future.await {
        Ok(value) => ok(value),
        Err(err) => fail(err),
    }
}
