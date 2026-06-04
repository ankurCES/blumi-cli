//! Dual memory (MEMORY/USER), SKILL.md skills, and self-evolution tools
//! (author skills, edit config, reload in place).

mod bundled;
mod catalog;
mod grid_status;
mod memory;
mod memory_tool;
mod reload;
mod restart_gateway;
pub mod self_config;
pub mod skill_manager;
mod skill_tool;

#[cfg(test)]
mod run_tests;

pub use bundled::{bundled_count, sync_bundled_skills};
pub use catalog::{Skill, SkillCatalog, SkillMeta};
pub use grid_status::GridStatusTool;
pub use memory::MemorySnapshot;
pub use memory_tool::MemoryTool;
pub use reload::ReloadTool;
pub use restart_gateway::RestartGatewayTool;
pub use self_config::SelfConfig;
pub use skill_manager::SkillManager;
pub use skill_tool::SkillTool;
