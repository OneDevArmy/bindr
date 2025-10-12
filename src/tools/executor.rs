use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::json;
use tokio::{fs, io::AsyncWriteExt, process::Command};

use crate::tools::{
    ApplyPatchOptions, CommandOptions, DiffFileOptions, ListDirectoryOptions, ReadFileOptions,
    ToolInvocation, WebSearchOptions, WriteFileOptions,
};

/// Result returned after a tool executes.
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub summary: String,
    pub output: Option<String>,
}

/// Executes validated tool invocations within the workspace root.
pub struct ToolExecutor {
    root: PathBuf,
}

impl ToolExecutor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn execute(&self, invocation: &ToolInvocation) -> Result<ToolExecutionResult> {
        match &invocation.tool {
            crate::tools::BindrTool::ReadFile(opts) => self.read_file(opts).await,
            crate::tools::BindrTool::WriteFile(opts) => self.write_file(opts).await,
            crate::tools::BindrTool::ListDirectory(opts) => self.list_directory(opts).await,
            crate::tools::BindrTool::DiffFile(opts) => self.diff_file(opts).await,
            crate::tools::BindrTool::ApplyPatch(opts) => self.apply_patch(opts).await,
            crate::tools::BindrTool::RunCommand(opts) => self.run_command(opts).await,
            crate::tools::BindrTool::ListModels | crate::tools::BindrTool::SelectModel(_) => {
                Err(anyhow!("model management delegated to runtime"))
            }
            crate::tools::BindrTool::WebSearch(opts) => self.web_search(opts).await,
        }
    }

    fn sanitize_path(&self, path: &Path) -> Result<PathBuf> {
        if path.is_absolute() {
            return Err(anyhow!("absolute paths are not permitted"));
        }

        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(anyhow!("path may not traverse outside the workspace"));
        }

        Ok(self.root.join(path))
    }

    async fn read_file(&self, opts: &ReadFileOptions) -> Result<ToolExecutionResult> {
        let path = self.sanitize_path(&opts.path)?;
        let bytes = fs::read(&path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut content = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(limit) = opts.max_bytes
            && content.len() > limit
        {
            content.truncate(limit);
            content.push_str("\n… [truncated]");
        }
        Ok(ToolExecutionResult {
            summary: format!("Read {} bytes from {}", content.len(), opts.path.display()),
            output: Some(content),
        })
    }

    async fn list_directory(&self, opts: &ListDirectoryOptions) -> Result<ToolExecutionResult> {
        let path = self.sanitize_path(&opts.path)?;
        if opts.recursive {
            return Err(anyhow!("recursive directory listing is not yet supported"));
        }
        let mut entries = fs::read_dir(&path)
            .await
            .with_context(|| format!("failed to read directory {}", path.display()))?;

        let mut lines = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if !opts.include_hidden
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }

            let file_type = entry.file_type().await?;
            let mut name = entry.file_name().to_string_lossy().to_string();
            if file_type.is_dir() {
                name.push('/');
            }
            lines.push(name);

            if let Some(max) = opts.max_entries
                && lines.len() >= max
            {
                lines.push("…".to_string());
                break;
            }
        }

        Ok(ToolExecutionResult {
            summary: format!("Listed {} entries in {}", lines.len(), opts.path.display()),
            output: Some(lines.join("\n")),
        })
    }

    async fn write_file(&self, opts: &WriteFileOptions) -> Result<ToolExecutionResult> {
        let path = self.sanitize_path(&opts.path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to ensure directory {}", parent.display()))?;
        }

        if !opts.create_if_missing && !path.exists() {
            return Err(anyhow!("file {} does not exist", opts.path.display()));
        }

        fs::write(&path, opts.contents.as_bytes())
            .await
            .with_context(|| format!("failed to write {}", path.display()))?;

        Ok(ToolExecutionResult {
            summary: format!(
                "Wrote {} bytes to {}",
                opts.contents.len(),
                opts.path.display()
            ),
            output: None,
        })
    }

    async fn diff_file(&self, opts: &DiffFileOptions) -> Result<ToolExecutionResult> {
        let path = self.sanitize_path(&opts.path)?;
        let output = Command::new("git")
            .arg("diff")
            .arg(format!("-U{}", opts.context_lines))
            .arg("--")
            .arg(path.strip_prefix(&self.root).unwrap_or(&path))
            .current_dir(&self.root)
            .output()
            .await
            .with_context(|| "failed to launch git diff")?;

        if !output.status.success() {
            return Err(anyhow!("git diff exited with status {}", output.status));
        }

        let diff = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(ToolExecutionResult {
            summary: format!("Generated diff for {}", opts.path.display()),
            output: Some(diff),
        })
    }

    async fn apply_patch(&self, opts: &ApplyPatchOptions) -> Result<ToolExecutionResult> {
        use std::process::Stdio;

        let mut child = Command::new("patch")
            .arg("-p0")
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| "failed to spawn patch command")?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(opts.patch.as_bytes())
                .await
                .with_context(|| "failed to write patch to stdin")?;
        }

        let output = child
            .wait_with_output()
            .await
            .with_context(|| "failed to apply patch")?;

        if !output.status.success() {
            return Err(anyhow!(
                "patch command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(ToolExecutionResult {
            summary: "Patch applied successfully".to_string(),
            output: Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        })
    }

    async fn run_command(&self, opts: &CommandOptions) -> Result<ToolExecutionResult> {
        let working_dir = self.sanitize_path(&opts.working_dir)?;

        let output = Command::new(&opts.command)
            .args(&opts.args)
            .current_dir(working_dir)
            .output()
            .await
            .with_context(|| format!("failed to run command {}", opts.command))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if !output.status.success() {
            return Err(anyhow!(
                "command exited with code {}\n{}",
                output.status,
                stderr
            ));
        }

        let mut combined = stdout;
        if !stderr.trim().is_empty() {
            combined.push_str("\n--- stderr ---\n");
            combined.push_str(&stderr);
        }

        Ok(ToolExecutionResult {
            summary: format!("Command `{}` executed", opts.command),
            output: Some(combined),
        })
    }

    async fn web_search(&self, opts: &WebSearchOptions) -> Result<ToolExecutionResult> {
        let api_key = std::env::var("TAVILY_API_KEY")
            .map_err(|_| anyhow!("TAVILY_API_KEY is not set. Please export your Tavily key."))?;

        let client = reqwest::Client::new();
        let max_results = opts.max_results.unwrap_or(5).clamp(1, 10);
        let response = client
            .post("https://api.tavily.com/search")
            .json(&json!({
                "api_key": api_key,
                "query": opts.query,
                "max_results": max_results,
                "search_depth": "advanced",
            }))
            .send()
            .await
            .context("failed to call Tavily search API")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "tavily search failed with status {}",
                response.status()
            ));
        }

        let payload: TavilyResponse = response
            .json()
            .await
            .context("failed to parse Tavily response")?;

        if payload.results.is_empty() {
            return Ok(ToolExecutionResult {
                summary: "Search returned no results".to_string(),
                output: Some("No documents found.".to_string()),
            });
        }

        let mut lines = Vec::new();
        for (idx, item) in payload
            .results
            .iter()
            .take(max_results as usize)
            .enumerate()
        {
            lines.push(format!(
                "{}. {}\n{}\n{}\n",
                idx + 1,
                item.title,
                item.url,
                item.content.trim()
            ));
        }

        Ok(ToolExecutionResult {
            summary: format!("Found {} search results", payload.results.len()),
            output: Some(lines.join("\n")),
        })
    }
}

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
}
