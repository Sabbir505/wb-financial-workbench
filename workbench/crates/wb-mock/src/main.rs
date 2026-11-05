//! wb-mock CLI: run a standalone mock KG-Palantir research service.
//!
//! Usage:
//!   wb-mock [--port 8012] [--api-mock <dir>] [--vault <dir>] [--webhook-url <url>]
//!
//! Dev defaults: port 8012, api-mock `vendor/api_mock` (repo-relative), vault
//! `research-vault`, no webhook target. Point WB at it via wb.config.json or env.

use wb_mock::{MockConfig, MockServer};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().expect("cwd");
    let api_mock = arg_value(&args, "--api-mock").unwrap_or_else(|| {
        // Prefer the repo vendored fixtures.
        let candidates = [
            cwd.join("vendor/api_mock"),
            cwd.join("../../vendor/api_mock"),
            cwd.join("../vendor/api_mock"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| cwd.join("vendor/api_mock"))
            .to_string_lossy()
            .to_string()
    });
    let vault = arg_value(&args, "--vault")
        .unwrap_or_else(|| cwd.join("research-vault").to_string_lossy().to_string());
    let port: u16 = arg_value(&args, "--port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(8012);
    let webhook_target = arg_value(&args, "--webhook-url");

    let cfg = MockConfig {
        port,
        api_mock_dir: api_mock.clone().into(),
        vault_root: vault.clone().into(),
        webhook_target,
    };
    let server = MockServer::spawn(cfg).expect("mock server failed to start");
    println!("wb-mock listening on {}", server.base_url);
    println!("  MCP endpoint   : {}/mcp", server.base_url);
    println!("  read-only base : {}/api", server.base_url);
    println!("  api fixtures   : {api_mock}");
    println!("  vault root     : {vault}");
    println!("  press Ctrl+C to stop");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
