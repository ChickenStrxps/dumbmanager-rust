//! Console entry point; stdout is reserved for MCP JSON-RPC.
fn main() {
    if let Err(error) = dumbmanager_rust_lib::run_mcp() {
        eprintln!("DumbManager MCP: {error}");
        std::process::exit(1);
    }
}
