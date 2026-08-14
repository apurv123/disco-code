//! End-to-end MCP stdio verification.
//!
//! CP-F: the MCP machinery (`mcp_stdio`, `mcp_tool_bridge`, `mcp_lifecycle_hardened`)
//! was inherited wholesale and had unit coverage, but nothing proved that a *real*
//! third-party MCP server could be spawned, handshaked, and have its tools discovered.
//! These tests close that gap.
//!
//! The Playwright test is `#[ignore]`d because it shells out to `npx` and needs
//! network access on first run. Run it explicitly with:
//!
//! ```text
//! cargo test -p runtime --test mcp_stdio_e2e -- --ignored --nocapture
//! ```

use std::fs;
use std::path::Path;

use runtime::{ConfigLoader, McpServerManager};

/// Write a `.claw/settings.json` containing `mcp_servers` and load it the same way
/// the CLI does, so the test exercises real config discovery rather than a
/// hand-built config struct.
fn manager_for(dir: &Path, mcp_servers_json: &str) -> McpServerManager {
    let claw_dir = dir.join(".claw");
    fs::create_dir_all(&claw_dir).expect("create .claw");
    fs::write(
        claw_dir.join("settings.json"),
        format!("{{\"mcpServers\":{mcp_servers_json}}}"),
    )
    .expect("write settings.json");

    let config = ConfigLoader::new(dir, dir.join("config-home"))
        .load()
        .expect("config loads");
    McpServerManager::from_runtime_config(&config)
}

#[test]
fn discovery_reports_failure_without_hanging_when_server_binary_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut manager = manager_for(
        temp.path(),
        r#"{"ghost":{"command":"disco-code-no-such-mcp-binary","args":[]}}"#,
    );

    assert_eq!(manager.server_names(), vec!["ghost".to_string()]);

    let report = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(manager.discover_tools_best_effort());

    // Best-effort discovery must degrade, not abort: a broken server is reported
    // and the session continues. This is the property the agent loop depends on.
    assert!(
        report.tools.is_empty(),
        "a nonexistent binary must not yield tools, got {:?}",
        report.tools
    );
    assert_eq!(
        report.failed_servers.len(),
        1,
        "expected exactly one failed server, got {:?}",
        report.failed_servers
    );
    assert_eq!(report.failed_servers[0].server_name, "ghost");
}

#[test]
fn multiple_servers_are_isolated_so_one_failure_does_not_suppress_the_others() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut manager = manager_for(
        temp.path(),
        r#"{
            "ghost_a":{"command":"disco-code-no-such-mcp-binary-a","args":[]},
            "ghost_b":{"command":"disco-code-no-such-mcp-binary-b","args":[]}
        }"#,
    );

    let mut names = manager.server_names();
    names.sort();
    assert_eq!(names, vec!["ghost_a".to_string(), "ghost_b".to_string()]);

    let report = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(manager.discover_tools_best_effort());

    assert_eq!(
        report.failed_servers.len(),
        2,
        "each server must be reported independently, got {:?}",
        report.failed_servers
    );
}

/// Live proof that a real, third-party MCP server works end to end.
///
/// Spawns `npx -y @playwright/mcp@latest`, performs the JSON-RPC `initialize`
/// handshake, and asserts that recognisable browser-automation tools come back.
#[test]
#[ignore = "spawns npx and requires network access on first run"]
fn playwright_mcp_server_is_spawned_and_its_tools_are_discovered() {
    let temp = tempfile::tempdir().expect("tempdir");
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    let mut manager = manager_for(
        temp.path(),
        &format!(r#"{{"playwright":{{"command":"{npx}","args":["-y","@playwright/mcp@latest"]}}}}"#),
    );

    let report = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(manager.discover_tools_best_effort());

    assert!(
        report.failed_servers.is_empty(),
        "playwright MCP failed to start: {:?}",
        report.failed_servers
    );

    let tool_names: Vec<String> = report
        .tools
        .iter()
        .map(|tool| tool.qualified_name.clone())
        .collect();
    println!("discovered {} playwright tools: {tool_names:#?}", tool_names.len());

    assert!(
        !tool_names.is_empty(),
        "playwright MCP advertised no tools at all"
    );
    // Namespacing matters: unqualified names would collide with built-in tools.
    assert!(
        tool_names.iter().all(|name| name.starts_with("mcp__playwright__")),
        "every discovered tool must be namespaced to its server, got {tool_names:?}"
    );
    assert!(
        tool_names.iter().any(|name| name.contains("navigate")),
        "expected a navigation tool among {tool_names:?}"
    );
}
