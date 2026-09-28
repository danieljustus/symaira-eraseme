pub(crate) mod handler {
    pub trait ToolHandler: Send + Sync {}
}

pub(crate) mod tools_list {
    pub enum ToolsListOutcome {
        Response(Vec<u8>),
        Notification,
        ParseError,
    }

    pub fn tools_list(_: &[u8]) -> ToolsListOutcome {
        ToolsListOutcome::ParseError
    }
}

pub(crate) mod tools_call {
    use super::handler::ToolHandler;
    use symeraseme_core::llm::CancellationToken;

    pub enum ToolsCallOutcome {
        Response(Vec<u8>),
        Notification,
        ParseError,
    }

    pub fn tools_call_cancellable(
        _: &[u8],
        _: &dyn ToolHandler,
        _: &CancellationToken,
    ) -> ToolsCallOutcome {
        ToolsCallOutcome::ParseError
    }

    pub fn legacy_redact_file(_: &[u8], _: &dyn ToolHandler) -> ToolsCallOutcome {
        ToolsCallOutcome::ParseError
    }
}

#[path = "../../../crates/symeraseme-cli/src/mcp/protocol.rs"]
pub(crate) mod protocol;
#[path = "../../../crates/symeraseme-cli/src/mcp/stream.rs"]
pub(crate) mod stream;
