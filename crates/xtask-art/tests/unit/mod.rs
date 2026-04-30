// Test binary: `unwrap()` is the idiomatic assertion here, and a panic is the
// failure report. The workspace forbids it in real code, where a panic is a
// crash.
#![allow(clippy::unwrap_used)]

mod support;
mod test_bake_stage;

mod test_cli;
mod test_cli_commands;
mod test_cli_prompts;
mod test_cli_run;
mod test_library;
mod test_lock;
mod test_meshy;
mod test_meshy_client;
mod test_openai;
mod test_openai_client;
mod test_pack;
mod test_pack_stage;
mod test_preview;
mod test_spec;
mod test_stages;
mod test_stages_run;
