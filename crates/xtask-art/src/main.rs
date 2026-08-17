//! Thin entry point. Everything lives in the library, so the tests under
//! `tests/` can reach it — an integration test cannot import a binary.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    xtask_art::cli::run_from_args(std::env::args_os()).await
}
