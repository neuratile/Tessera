//! Jira push service — handles pushing artifacts and test cases to Jira.

use serde::Serialize;
use sqlx::SqlitePool;

use std::fmt::Write;

use crate::error::AppResult;
use crate::providers::trackers::NewIssue;
use crate::repositories::external_link_repo::{self, ExternalLinkRow, ExternalLinkUpsert};
use crate::repositories::tracker_config_repo;
use crate::services::tracker_config_service::build_tracker_client;
use crate::utils::crypto::CryptoKey;

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub keys: Vec<String>,
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkPushResultItem {
    pub artifact_id: String,
    pub success: bool,
    pub keys: Vec<String>,
    pub error: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct TestCasesStructuredData {
    cases: Vec<TestCaseItem>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TestCaseItem {
    id: String,
    title: String,
    preconditions: Option<Vec<String>>,
    steps: Vec<String>,
    expected_result: Option<String>,
    priority: Option<String>,
}

/// Push an artifact to Jira.
#[allow(clippy::too_many_lines)]
pub async fn push_artifact(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    artifact_id: &str,
) -> AppResult<PushResult> {
    let tracker_config = tracker_config_repo::fetch_active(pool, DEFAULT_USER_ID, "jira").await?;
    let tracker = build_tracker_client(crypto, &tracker_config)?;
    let artifact = crate::repositories::artifact_repo::fetch(pool, artifact_id).await?;

    let mut keys = Vec::new();
    let mut urls = Vec::new();

    match artifact.artifact_type {
        crate::repositories::artifact_repo::ArtifactType::TestPlan => {
            // Push as Epic
            let new_issue = NewIssue {
                project_key: tracker_config.project_key.clone(),
                summary: artifact.title.clone(),
                description: artifact.content_md.clone(),
                issue_type: "Epic".to_string(),
                priority: None,
                labels: vec!["tessera-test-plan".to_string()],
                parent_key: None,
            };

            let created = tracker.create_issue(new_issue).await?;
            external_link_repo::upsert(
                pool,
                ExternalLinkUpsert {
                    artifact_id: artifact.id.clone(),
                    tracker: "jira".to_string(),
                    item_ref: String::new(),
                    issue_key: created.key.clone(),
                    issue_url: created.url.clone(),
                    issue_type: Some(created.issue_type.clone()),
                    last_status: Some(created.status.clone()),
                },
            )
            .await?;

            keys.push(created.key);
            urls.push(created.url);
        }
        crate::repositories::artifact_repo::ArtifactType::TestCases => {
            // Push individual test cases
            let structured: TestCasesStructuredData = serde_json::from_value(artifact.structured_data.clone())?;

            // Check parent Epic
            let mut parent_epic_key: Option<String> = None;
            if let Some(ref parent_id) = artifact.parent_id {
                if let Some(link) = external_link_repo::fetch_for_item(pool, parent_id, "jira", "").await? {
                    parent_epic_key = Some(link.issue_key);
                }
            }

            for case in structured.cases {
                let mut description = String::new();
                if let Some(pre) = &case.preconditions {
                    if !pre.is_empty() {
                        description.push_str("h3. Preconditions\n");
                        for p in pre {
                            let _ = writeln!(description, "* {p}");
                        }
                        description.push('\n');
                    }
                }
                description.push_str("h3. Steps\n");
                for (i, step) in case.steps.iter().enumerate() {
                    let _ = writeln!(description, "{}. {step}", i + 1);
                }
                description.push('\n');
                if let Some(exp) = &case.expected_result {
                    description.push_str("h3. Expected Result\n");
                    description.push_str(exp);
                    description.push('\n');
                }

                // Map priority
                let jira_priority = match case.priority.as_deref() {
                    Some("p0") => Some("High".to_string()),
                    Some("p1") => Some("Medium".to_string()),
                    Some("p2") => Some("Low".to_string()),
                    Some("p3") => Some("Lowest".to_string()),
                    other => other.map(String::from),
                };

                let new_issue = NewIssue {
                    project_key: tracker_config.project_key.clone(),
                    summary: case.title.clone(),
                    description,
                    issue_type: tracker_config.issue_type.clone(),
                    priority: jira_priority,
                    labels: vec!["tessera-test-case".to_string()],
                    parent_key: parent_epic_key.clone(),
                };

                let created = tracker.create_issue(new_issue).await?;
                external_link_repo::upsert(
                    pool,
                    ExternalLinkUpsert {
                        artifact_id: artifact.id.clone(),
                        tracker: "jira".to_string(),
                        item_ref: case.id.clone(),
                        issue_key: created.key.clone(),
                        issue_url: created.url.clone(),
                        issue_type: Some(created.issue_type.clone()),
                        last_status: Some(created.status.clone()),
                    },
                )
                .await?;

                keys.push(created.key);
                urls.push(created.url);
            }
        }
        _ => {
            // Push other artifacts (e.g. bug_report, defect_report, context_md) as the default issue type
            let new_issue = NewIssue {
                project_key: tracker_config.project_key.clone(),
                summary: artifact.title.clone(),
                description: artifact.content_md.clone(),
                issue_type: tracker_config.issue_type.clone(),
                priority: None,
                labels: vec![format!("tessera-{}", artifact.artifact_type.as_str())],
                parent_key: None,
            };

            let created = tracker.create_issue(new_issue).await?;
            external_link_repo::upsert(
                pool,
                ExternalLinkUpsert {
                    artifact_id: artifact.id.clone(),
                    tracker: "jira".to_string(),
                    item_ref: String::new(),
                    issue_key: created.key.clone(),
                    issue_url: created.url.clone(),
                    issue_type: Some(created.issue_type.clone()),
                    last_status: Some(created.status.clone()),
                },
            )
            .await?;

            keys.push(created.key);
            urls.push(created.url);
        }
    }

    Ok(PushResult { keys, urls })
}

/// Bulk push multiple artifacts to Jira.
pub async fn bulk_push_artifacts(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    artifact_ids: Vec<String>,
) -> AppResult<Vec<BulkPushResultItem>> {
    let mut results = Vec::new();
    for id in artifact_ids {
        match push_artifact(pool, crypto, &id).await {
            Ok(res) => {
                results.push(BulkPushResultItem {
                    artifact_id: id,
                    success: true,
                    keys: res.keys,
                    error: None,
                });
            }
            Err(e) => {
                results.push(BulkPushResultItem {
                    artifact_id: id,
                    success: false,
                    keys: vec![],
                    error: Some(e.to_string()),
                });
            }
        }
    }
    Ok(results)
}

/// Refresh the status of a linked Jira issue and return the updated link row
/// so the UI can patch its artifact→link map without a second round-trip.
pub async fn refresh_link_status(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    link_id: &str,
) -> AppResult<ExternalLinkRow> {
    let link = external_link_repo::fetch(pool, link_id).await?;
    let tracker_config = tracker_config_repo::fetch_active(pool, DEFAULT_USER_ID, &link.tracker).await?;
    let tracker = build_tracker_client(crypto, &tracker_config)?;
    let status = tracker
        .get_issue_status(&link.issue_key)
        .await?;
    external_link_repo::update_status(pool, link_id, &status).await?;
    external_link_repo::fetch(pool, link_id).await
}

/// Post sandbox run results as a comment on any linked issues for this artifact.
pub async fn post_run_comment(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    artifact_id: &str,
    status: &str,
    passed_count: u32,
    failed_count: u32,
) -> AppResult<()> {
    let config_opt = tracker_config_repo::fetch_for_user_tracker(pool, DEFAULT_USER_ID, "jira").await?;
    let tracker_config = match config_opt {
        Some(c) if c.is_active => c,
        _ => return Ok(()),
    };

    let tracker = build_tracker_client(crypto, &tracker_config)?;

    let links = external_link_repo::list_for_artifact(pool, artifact_id).await?;
    if links.is_empty() {
        return Ok(());
    }

    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let body = format!(
        "Automated run {}: {} — {}/{} passed",
        date,
        status.to_uppercase(),
        passed_count,
        passed_count + failed_count
    );

    for link in links {
        let _ = tracker.add_comment(&link.issue_key, &body).await;
    }

    Ok(())
}

