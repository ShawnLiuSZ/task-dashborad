// 避免打包后出现额外的控制台窗口（仅对 Windows 生效，macOS 无影响）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 作为 MCP server 运行时（被 claude-code / codex / WorkBuddy 等以 stdio 拉起），
    // 不启动 GUI，直接走 JSON-RPC 循环。
    if std::env::args().any(|a| a == "mcp") {
        taskboard_lib::run_mcp();
    } else {
        taskboard_lib::run();
    }
}
