#![warn(clippy::pedantic, clippy::nursery)]

pub mod acp_client;
pub mod agent_pool;
pub mod attention;
pub mod causal;
pub mod confidence;
pub mod context;
pub mod guardrails;
pub mod llm;
pub mod mcp;
pub mod memory;
pub mod meta;
pub mod observability;
pub mod orchestrator;
pub mod plugin;
pub mod reasoning;
pub mod risk;
pub mod security;
pub mod skills;
pub mod tools;
pub mod working_memory;

#[cfg(test)]
mod tests;
