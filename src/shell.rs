use std::sync::Arc;
use std::time::Instant;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::log::{LogEntry, LogStore};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommandRequest {
    #[schemars(description = "The shell command to execute")]
    pub command: String,
    #[schemars(description = "Working directory for the command (optional)")]
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScriptRequest {
    #[schemars(description = "The shell script content to execute (can be multi-line)")]
    pub script: String,
    #[schemars(description = "Working directory for the script (optional)")]
    pub cwd: Option<String>,
}

#[derive(Clone)]
pub struct AlpineShell {
    log_store: Arc<LogStore>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AlpineShell {
    pub fn new(log_store: Arc<LogStore>) -> Self {
        Self {
            log_store,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Execute a shell command and return its output (stdout, stderr, exit code)")]
    async fn execute_command(
        &self,
        Parameters(req): Parameters<CommandRequest>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(&req.command);
        if let Some(cwd) = &req.cwd {
            cmd.current_dir(cwd);
        }
        let output = cmd.output().await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (result, exit_code, stdout_str, stderr_str) = match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let code = out.status.code().unwrap_or(-1);
                let mut text = stdout.clone();
                if !stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[stderr]\n");
                    text.push_str(&stderr);
                }
                text.push_str(&format!("\n[exit code: {}]", code));
                if out.status.success() {
                    (CallToolResult::success(vec![Content::text(text)]), code, stdout, stderr)
                } else {
                    (CallToolResult::error(vec![Content::text(text)]), code, stdout, stderr)
                }
            }
            Err(e) => (
                CallToolResult::error(vec![Content::text(e.to_string())]),
                -2,
                String::new(),
                e.to_string(),
            ),
        };

        self.log_store.push(LogEntry {
            id: 0,
            time: chrono::Utc::now().to_rfc3339(),
            command: req.command.clone(),
            stdout: stdout_str,
            stderr: stderr_str,
            exit_code,
            duration_ms,
        });

        Ok(result)
    }

    #[tool(description = "Execute a multi-line shell script and return its output")]
    async fn execute_script(
        &self,
        Parameters(req): Parameters<ScriptRequest>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();

        let mut cmd = tokio::process::Command::new("/bin/sh");
        if let Some(cwd) = &req.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let (exit_code, stdout_str, stderr_str, result) = match cmd.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(req.script.as_bytes()).await;
                    let _ = stdin.shutdown().await;
                }
                match child.wait_with_output().await {
                    Ok(out) => {
                        let so = String::from_utf8_lossy(&out.stdout).to_string();
                        let se = String::from_utf8_lossy(&out.stderr).to_string();
                        let code = out.status.code().unwrap_or(-1);
                        let mut text = so.clone();
                        if !se.is_empty() {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str("[stderr]\n");
                            text.push_str(&se);
                        }
                        text.push_str(&format!("\n[exit code: {}]", code));
                        let r = if out.status.success() {
                            CallToolResult::success(vec![Content::text(text)])
                        } else {
                            CallToolResult::error(vec![Content::text(text)])
                        };
                        (code, so, se, r)
                    }
                    Err(e) => (-2, String::new(), e.to_string(), CallToolResult::error(vec![Content::text(e.to_string())])),
                }
            }
            Err(e) => (-3, String::new(), e.to_string(), CallToolResult::error(vec![Content::text(e.to_string())])),
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        self.log_store.push(LogEntry {
            id: 0,
            time: chrono::Utc::now().to_rfc3339(),
            command: req.script.clone(),
            stdout: stdout_str,
            stderr: stderr_str,
            exit_code,
            duration_ms,
        });

        Ok(result)
    }
}

#[tool_handler(instructions = "Alpine Linux shell server. Execute commands and scripts, returns stdout, stderr and exit code.")]
impl ServerHandler for AlpineShell {}
