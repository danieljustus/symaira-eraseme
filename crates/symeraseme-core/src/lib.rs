#![deny(unsafe_code)]

//! Deterministic domain foundations for Symaira EraseMe.

#[cfg(test)]
#[path = "../../symeraseme-cli/tests/support/go_source_pin.rs"]
mod go_source_pin;

pub mod campaign;
pub mod config;
pub mod confirmation;
pub mod deadlines;
pub mod email;
pub mod identity;
pub mod jsonorder;
pub mod llm;
pub mod manualtasks;
pub mod redaction;
pub mod registry;
pub mod reporting;
pub mod storage;
pub mod templating;
pub mod timeutil;
pub mod triage_contract;
pub mod triage_prompts;
pub mod triage_service;
pub mod version;
