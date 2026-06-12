use std::sync::Arc;
use std::time::{Duration, Instant};

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
use tokio::io::AsyncWriteExt;

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodeRequest {
    #[schemars(description = "The source code to execute")]
    pub code: String,
    #[schemars(description = "Working directory (optional)")]
    pub cwd: Option<String>,
    #[schemars(description = "Timeout in seconds (default 30, max 120)")]
    pub timeout: Option<u64>,
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

    #[tool(
        description = "Execute a shell command and return its output (stdout, stderr, exit code)"
    )]
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
                    (
                        CallToolResult::success(vec![Content::text(text)]),
                        code,
                        stdout,
                        stderr,
                    )
                } else {
                    (
                        CallToolResult::error(vec![Content::text(text)]),
                        code,
                        stdout,
                        stderr,
                    )
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
                    Err(e) => (
                        -2,
                        String::new(),
                        e.to_string(),
                        CallToolResult::error(vec![Content::text(e.to_string())]),
                    ),
                }
            }
            Err(e) => (
                -3,
                String::new(),
                e.to_string(),
                CallToolResult::error(vec![Content::text(e.to_string())]),
            ),
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

    #[tool(
        description = "Execute Python code snippet, returns stdout, stderr and exit code. Suitable for computation, data analysis, scripting tasks. Timeout defaults to 30s, max 120s."
    )]
    async fn run_python(
        &self,
        Parameters(req): Parameters<CodeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        let timeout_secs = req.timeout.unwrap_or(30).min(120);

        let tmp_path = format!(
            "/tmp/mcp_py_{}.py",
            std::process::id() as u64 * 1_000_000 + start.elapsed().as_micros() as u64
        );
        if let Err(e) = tokio::fs::write(&tmp_path, &req.code).await {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "failed to write temp file: {e}"
            ))]));
        }

        let mut cmd = tokio::process::Command::new("python3");
        cmd.arg("-u").arg(&tmp_path);
        if let Some(cwd) = &req.cwd {
            cmd.current_dir(cwd);
        }

        let (exit_code, stdout_str, stderr_str, result) =
            run_with_output(cmd, Duration::from_secs(timeout_secs)).await;

        let _ = tokio::fs::remove_file(&tmp_path).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        self.log_store.push(LogEntry {
            id: 0,
            time: chrono::Utc::now().to_rfc3339(),
            command: format!("[python]\n{}", req.code),
            stdout: stdout_str,
            stderr: stderr_str,
            exit_code,
            duration_ms,
        });

        Ok(result)
    }

    #[tool(
        description = "Execute Node.js code snippet, returns stdout, stderr and exit code. Supports ES modules (import/export). Timeout defaults to 30s, max 120s."
    )]
    async fn run_node(
        &self,
        Parameters(req): Parameters<CodeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let start = Instant::now();
        let timeout_secs = req.timeout.unwrap_or(30).min(120);

        let tmp_path = format!(
            "/tmp/mcp_js_{}.mjs",
            std::process::id() as u64 * 1_000_000 + start.elapsed().as_micros() as u64
        );
        if let Err(e) = tokio::fs::write(&tmp_path, &req.code).await {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "failed to write temp file: {e}"
            ))]));
        }

        let mut cmd = tokio::process::Command::new("node");
        cmd.arg(&tmp_path);
        if let Some(cwd) = &req.cwd {
            cmd.current_dir(cwd);
        }

        let (exit_code, stdout_str, stderr_str, result) =
            run_with_output(cmd, Duration::from_secs(timeout_secs)).await;

        let _ = tokio::fs::remove_file(&tmp_path).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        self.log_store.push(LogEntry {
            id: 0,
            time: chrono::Utc::now().to_rfc3339(),
            command: format!("[node]\n{}", req.code),
            stdout: stdout_str,
            stderr: stderr_str,
            exit_code,
            duration_ms,
        });

        Ok(result)
    }
}

async fn run_with_output(
    mut cmd: tokio::process::Command,
    timeout: Duration,
) -> (i32, String, String, CallToolResult) {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return (
                -3,
                String::new(),
                e.to_string(),
                CallToolResult::error(vec![Content::text(e.to_string())]),
            );
        }
    };

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => {
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
        Ok(Err(e)) => (
            -2,
            String::new(),
            e.to_string(),
            CallToolResult::error(vec![Content::text(e.to_string())]),
        ),
        Err(_) => {
            let msg = format!("timeout after {}s", timeout.as_secs());
            (
                -4,
                String::new(),
                msg.clone(),
                CallToolResult::error(vec![Content::text(msg)]),
            )
        }
    }
}

#[tool_handler(
    instructions = "Alpine Linux shell server. Execute commands and scripts, returns stdout, stderr and exit code."
)]
impl ServerHandler for AlpineShell {}
