//! Built-in tools for the blumi agent.
//!
//! Each tool implements `blumi_core::TypedTool` and is wrapped in `Typed` to
//! become a `Tool`. [`register_builtin_tools`] installs the default set into a
//! registry; the generic execution pipeline lives in `blumi-core`.

mod dir;
mod files;
mod path;
mod search;
mod shell;
mod todo;

pub use dir::ListDirectory;
pub use files::{FileEdit, FileRead, FileWrite};
pub use search::{Glob, Grep};
pub use shell::Bash;
pub use todo::TodoWrite;

use blumi_core::{ToolRegistry, Typed};
use std::sync::Arc;

/// Register every built-in tool into `reg`.
pub fn register_builtin_tools(reg: &mut ToolRegistry) {
    reg.register(Arc::new(Typed(FileRead)));
    reg.register(Arc::new(Typed(FileWrite)));
    reg.register(Arc::new(Typed(FileEdit)));
    reg.register(Arc::new(Typed(ListDirectory)));
    reg.register(Arc::new(Typed(Bash)));
    reg.register(Arc::new(Typed(Glob)));
    reg.register(Arc::new(Typed(Grep)));
    reg.register(Arc::new(Typed(TodoWrite)));
}

#[cfg(test)]
pub(crate) mod testutil {
    use blumi_core::{EventEmitter, Interactor, ToolContext};
    use blumi_exec::LocalExecutor;
    use blumi_protocol::SessionId;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// A ToolContext backed by a LocalExecutor with no live UI attached.
    pub fn ctx(working_dir: &Path) -> ToolContext {
        let (etx, _erx) = mpsc::unbounded_channel();
        let (itx, _irx) = mpsc::unbounded_channel();
        ToolContext {
            session_id: SessionId::from("test"),
            working_dir: working_dir.to_path_buf(),
            executor: Arc::new(LocalExecutor::new(working_dir)),
            events: EventEmitter::new(etx),
            interactor: Interactor::new(itx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_all_builtins() {
        let mut reg = ToolRegistry::new();
        register_builtin_tools(&mut reg);
        assert_eq!(reg.len(), 8);
        assert!(reg.get("Bash").is_some());
        // every non-deferred tool produces a spec
        assert_eq!(reg.specs().len(), 8);
    }
}
