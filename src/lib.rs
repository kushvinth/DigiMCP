//! DigiMCP library — core portal client, MCP server, tool registry, and CLI runtime.

pub mod cli;
pub mod config;
pub mod error;
pub mod journal;
pub mod login;
pub mod portal;
pub mod registry;
pub mod server;
pub mod setup;
pub mod simplify;
pub mod tools;
pub mod ui;

pub use cli::real_main;
