//! Thin entry point. Everything lives in the library, so the tests under
//! `tests/` can reach it, an integration test cannot import a binary.

fn main() -> anyhow::Result<()> {
    xtask_world::cli::run_from_args(std::env::args_os())
}
