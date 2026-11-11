//! `digimcp` binary entrypoint.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    digimcp::cli::real_main()
}
