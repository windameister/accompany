use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{AppHandle, Emitter};

use crate::brain::queue::{BrainEvent, EventQueue, EventSource, Priority};

const GITHUB_API: &str = "https://api.github.com";
const POLL_INTERVAL_SECS: u64 = 60;

/// Repos to monitor.
const WATCHED_REPOS: &[&str] = &[
    "windameister/cascade-strategy",
    "windameister/futures-data-pipeline",
    "windameister/auto-adaptive-vol",
];

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    id: i64,
    name: String,
    status: String,         // "completed", "in_progress", "queued"
    conclusion: Option<String>, // "success", "failure", "cancelled", etc.
    html_url: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RunsResponse {
    workflow_runs: Vec<WorkflowRun>,
}

/// Start polling GitHub Actions for watched repos.
pub async fn start_github_monitor(app: AppHandle, brain: EventQueue) {
    // Get GitHub token from gh CLI
    let token = match get_gh_token().await {
        Some(t) => t,
        None => {
            tracing::warn!("GitHub token not available (gh auth token failed). GitHub monitoring disabled.");
            return;
        }
    };

    tracing::info!(
        "GitHub monitor started, watching {} repos (poll every {}s)",
        WATCHED_REPOS.len(),
        POLL_INTERVAL_SECS
    );

    let http = Client::new();
    // Track last seen run ID per repo to detect new completions
    let seen: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));

    // Initial fetch to populate seen state (don't alert on startup)
    for repo in WATCHED_REPOS {
        if let Ok(runs) = fetch_recent_runs(&http, &token, repo).await {
            if let Some(latest) = runs.first() {
                seen.lock().await.insert(repo.to_string(), latest.id);
            }
        }
    }

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

        for repo in WATCHED_REPOS {
            let runs = match fetch_recent_runs(&http, &token, repo).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("Failed to fetch runs for {}: {}", repo, e);
                    continue;
                }
            };

            // Collect new runs under lock, then release before doing TTS
            let new_runs: Vec<(String, String, String)> = {
                let mut seen_lock = seen.lock().await;
                let last_seen_id = seen_lock.get(*repo).copied().unwrap_or(0);

                let mut new = Vec::new();
                for run in &runs {
                    if run.id <= last_seen_id {
                        break;
                    }
                    if run.status != "completed" {
                        continue;
                    }
                    let conclusion = run.conclusion.clone().unwrap_or_else(|| "unknown".into());
                    new.push((run.name.clone(), conclusion, run.html_url.clone()));
                }

                // Update last seen immediately
                if let Some(latest) = runs.first() {
                    seen_lock.insert(repo.to_string(), latest.id);
                }

                new
            }; // lock released

            // Process notifications without holding the lock
            let repo_short = repo.rsplit('/').next().unwrap_or(repo);
            for (name, conclusion, url) in &new_runs {
                let (msg, mood) = match conclusion.as_str() {
                    "success" => (
                        format!("{}的{}部署成功了喵~", repo_short, name),
                        "happy",
                    ),
                    "failure" => (
                        format!("{}的{}失败了！快看看喵！", repo_short, name),
                        "alert",
                    ),
                    "cancelled" => (
                        format!("{}的{}被取消了", repo_short, name),
                        "idle",
                    ),
                    _ => (
                        format!("{}的{}状态: {}", repo_short, name, conclusion),
                        "idle",
                    ),
                };

                tracing::info!("GitHub: {} - {} ({})", repo, name, conclusion);

                let (category, priority) = match conclusion.as_str() {
                    "failure" => ("deploy_failure", Priority::High),
                    "success" => ("deploy_success", Priority::Normal),
                    "cancelled" => ("deploy_cancelled", Priority::Low),
                    _ => ("deploy_other", Priority::Low),
                };

                let event = BrainEvent::new(EventSource::GitHub, priority, category, &msg)
                    .with_details(serde_json::json!({
                        "repo": repo, "name": name, "conclusion": conclusion, "url": url,
                    }))
                    .with_dedup(&format!("github_{}_{}", repo, category));

                brain.push(event).await;
            }

        }
    }
}

async fn fetch_recent_runs(
    http: &Client,
    token: &str,
    repo: &str,
) -> Result<Vec<WorkflowRun>, String> {
    let url = format!("{}/repos/{}/actions/runs?per_page=5", GITHUB_API, repo);

    let resp = http
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "Accompany/0.1")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API error: {}", resp.status()));
    }

    let body: RunsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse failed: {}", e))?;

    Ok(body.workflow_runs)
}

async fn get_gh_token() -> Option<String> {
    let output = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}
