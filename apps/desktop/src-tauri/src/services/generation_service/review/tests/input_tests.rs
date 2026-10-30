use super::*;
use crate::repositories::review_repo::{EvidenceState, ReviewSource};
use sha2::{Digest, Sha256};

fn mock() -> Mock {
    Mock {
        name: "ollama",
        tools: true,
        chunks: vec![],
        calls: Arc::new(AtomicUsize::new(0)),
        pending: false,
    }
}

fn source(blob: &CapturedBlob, side: SourceSide) -> ReviewSource {
    ReviewSource {
        path: blob.path.clone(),
        side,
        blob_id: blob.blob_id.clone(),
        mode: blob.mode.clone(),
        content_hash: blob.content_hash.clone(),
        byte_length: u32::try_from(blob.content.len()).unwrap(),
        line_count: u32::try_from(blob.content.split_inclusive(|b| *b == b'\n').count()).unwrap(),
    }
}

fn support_blob(content: &str) -> CapturedBlob {
    CapturedBlob {
        path: "src/support.ts".into(),
        blob_id: "b".repeat(40),
        mode: "100644".into(),
        content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
        content: content.as_bytes().to_vec(),
    }
}

#[test]
fn rejects_manifest_metadata_or_raw_identity_mismatch_before_streaming() {
    for field in [
        "mode",
        "blob",
        "hash",
        "length",
        "change",
        "snapshot",
        "fingerprint",
        "format",
        "base",
    ] {
        let (capture, support, mut manifest) = fixture();
        match field {
            "mode" => manifest.sources[0].mode = "100755".into(),
            "blob" => manifest.sources[0].blob_id = "d".repeat(40),
            "hash" => manifest.sources[0].content_hash = "d".repeat(64),
            "length" => manifest.sources[0].byte_length += 1,
            "change" => {
                manifest.changes.clear();
                manifest.context_paths.push("src/a.ts".into());
            }
            "snapshot" => manifest.snapshot_id = Uuid::new_v4(),
            "fingerprint" => manifest.index_fingerprint = "d".repeat(64),
            "format" => manifest.git_object_format = GitObjectFormat::Sha256,
            "base" => manifest.base_commit = Some("d".repeat(40)),
            _ => unreachable!(),
        }
        let request = ReviewRequest {
            provider_config_id: None,
            model: "pinned-model",
            capture: &capture,
            support: &support,
            manifest: &manifest,
        };
        assert!(prepare(&request, &mock()).is_err(), "{field}");
    }
}

#[test]
fn metadata_only_mode_and_rename_findings_need_no_fabricated_lines() {
    for renamed in [false, true] {
        let (mut capture, support, mut manifest) = fixture();
        let new = capture.files[0].new.as_mut().unwrap();
        new.mode = "100755".into();
        let old = CapturedBlob {
            path: if renamed {
                "src/before.ts".into()
            } else {
                new.path.clone()
            },
            blob_id: new.blob_id.clone(),
            mode: "100644".into(),
            content_hash: new.content_hash.clone(),
            content: new.content.clone(),
        };
        manifest.sources[0] = source(new, SourceSide::New);
        manifest.sources.push(source(&old, SourceSide::Old));
        manifest.changes[0] = ReviewChange {
            change_kind: if renamed {
                ReviewChangeKind::Renamed
            } else {
                ReviewChangeKind::Modified
            },
            old_path: Some(old.path.clone()),
            new_path: Some(new.path.clone()),
        };
        capture.files[0].old = Some(old);
        capture.files[0].hunks.clear();
        capture.files[0].change_kind = if renamed {
            ChangeKind::Renamed
        } else {
            ChangeKind::Modified
        };
        let request = ReviewRequest {
            provider_config_id: None,
            model: "pinned-model",
            capture: &capture,
            support: &support,
            manifest: &manifest,
        };
        let mock = mock();
        let mut context = prepare(&request, &mock).unwrap();
        let messages = fit(&request, &mut context, &mock, &review_v1::tool_schema()).unwrap();
        let wire = serde_json::to_string(&messages).unwrap();
        assert!(
            wire.contains("100755")
                && wire.contains("100644")
                && wire.contains("metadataOnlySources")
        );
        let mut payload = finding(&manifest);
        payload["findings"][0]["sources"][0]
            .as_object_mut()
            .unwrap()
            .remove("lineStart");
        payload["findings"][0]["sources"][0]
            .as_object_mut()
            .unwrap()
            .remove("lineEnd");
        // File evidence survives optional-source omission and never invents a range.
        context.excerpts.clear();
        let sources = metadata_sources(&request, &context);
        let findings = output::validate(
            &payload.to_string(),
            &review_v1::tool_schema(),
            &manifest,
            &context,
            &sources,
            "ollama",
        )
        .unwrap();
        assert_eq!(findings[0].sources[0].line_start, None);
        assert_eq!(findings[0].evidence_state, EvidenceState::Suspected);
        assert!(output::validate(
            &payload.to_string(),
            &review_v1::tool_schema(),
            &manifest,
            &context,
            &[],
            "ollama"
        )
        .is_err());
    }
}

#[test]
fn final_wrapper_drops_optional_support_and_revokes_its_citations() {
    let (capture, mut support, mut manifest) = fixture();
    // Quotes/backslashes expand in the serialized request. Raw-source-only
    // budgeting fits this, while the complete wrapper must omit it.
    let blob = support_blob(&format!("// {}\n", "\\\"".repeat(1300)));
    manifest.sources.push(source(&blob, SourceSide::New));
    manifest.context_paths.push(blob.path.clone());
    support.files.push(blob);
    let request = ReviewRequest {
        provider_config_id: None,
        model: "pinned-model",
        capture: &capture,
        support: &support,
        manifest: &manifest,
    };
    let mock = mock();
    let mut context = prepare(&request, &mock).unwrap();
    assert!(context.excerpts.iter().any(|e| e.path == "src/support.ts"));
    let messages = fit(&request, &mut context, &mock, &review_v1::tool_schema()).unwrap();
    assert!(
        !context.excerpts.iter().any(|e| e.path == "src/support.ts"),
        "final prompt cost: {}",
        context.input_tokens
    );
    assert!(context
        .omitted
        .iter()
        .any(|e| e.path == "src/support.ts" && e.reason == "prompt_budget"));
    assert!(context
        .truncation
        .iter()
        .any(|t| t.resource == "final_prompt" && t.omitted_count == 1));
    let wire = GenerateRequest {
        model: "pinned-model".into(),
        messages,
        tools: vec![review_v1::tool_schema()],
        temperature: Some(0.0),
        max_tokens: Some(6000),
        stop_sequences: vec![],
    };
    assert!(serde_json::to_string(&wire).unwrap().len() + 256 <= 8000);
    let mut payload = finding(&manifest);
    let citation = &mut payload["findings"][0]["sources"][0];
    citation["path"] = json!("src/support.ts");
    citation["blobId"] = json!(manifest.sources[1].blob_id);
    citation["contentHash"] = json!(manifest.sources[1].content_hash);
    citation["lineStart"] = json!(1);
    citation["lineEnd"] = json!(1);
    assert!(output::validate(
        &payload.to_string(),
        &review_v1::tool_schema(),
        &manifest,
        &context,
        &[],
        "ollama"
    )
    .is_err());
}

#[test]
fn oversized_required_code_fails_and_redaction_omissions_survive() {
    for content in [
        format!("const a = '{}';\n", "x".repeat(8000)),
        "const apiKey =\n 'synthetic-secret-value';\n".into(),
        "const token = [ // parts\n'opaque-credential-value'\n];\n".into(),
        "const token = [ // parts\r\n'opaque-credential-value'\r\n];\r\n".into(),
        "const token = ['first'\n, 'opaque-credential-value'\n];\n".into(),
    ] {
        let (mut capture, support, mut manifest) = fixture();
        let blob = capture.files[0].new.as_mut().unwrap();
        blob.content = content.as_bytes().to_vec();
        blob.content_hash = format!("{:x}", Sha256::digest(&blob.content));
        manifest.sources[0] = source(blob, SourceSide::New);
        capture.files[0].hunks[0].new_lines = manifest.sources[0].line_count;
        let request = ReviewRequest {
            provider_config_id: None,
            model: "pinned-model",
            capture: &capture,
            support: &support,
            manifest: &manifest,
        };
        let mock = mock();
        if content.len() > 8000 {
            assert!(matches!(
                prepare(&request, &mock),
                Err(AppError::LimitExceeded(_))
            ));
        } else {
            let mut context = prepare(&request, &mock).unwrap();
            let messages = fit(&request, &mut context, &mock, &review_v1::tool_schema()).unwrap();
            assert!(!serde_json::to_string(&messages)
                .unwrap()
                .contains("opaque-credential-value"));
            assert!(context.excerpts.is_empty());
            assert!(context
                .omitted
                .iter()
                .any(|e| e.path == "src/a.ts" && e.reason == "redaction_unmappable"));
        }
    }
}

#[test]
fn credential_continuations_never_reach_final_model_context() {
    for newline in ["\n", "\r\n"] {
        for (declaration, continuation) in [
            ("const token = [ // parts", "'opaque-credential-value'\n];"),
            (
                "const token = [ /* parts */",
                "'opaque-credential-value'\n];",
            ),
            ("const token = [ /* parts", "'opaque-credential-value'\n];"),
            ("const token = ''", "+ 'opaque-credential-value';"),
            ("const token", "= 'opaque-credential-value';"),
        ] {
            let (capture, mut support, mut manifest) = fixture();
            let raw = format!("{declaration}\n{continuation}\n").replace('\n', newline);
            let blob = support_blob(&raw);
            manifest.sources.push(source(&blob, SourceSide::New));
            manifest.context_paths.push(blob.path.clone());
            support.files.push(blob);
            let request = ReviewRequest {
                provider_config_id: None,
                model: "pinned-model",
                capture: &capture,
                support: &support,
                manifest: &manifest,
            };
            let mock = mock();
            let mut context = prepare(&request, &mock).unwrap();
            let messages = fit(&request, &mut context, &mock, &review_v1::tool_schema()).unwrap();
            assert!(!context.model_text().contains("opaque-credential-value"));
            assert!(!serde_json::to_string(&messages)
                .unwrap()
                .contains("opaque-credential-value"));
            assert!(context
                .omitted
                .iter()
                .any(|e| e.path == "src/support.ts" && e.reason == "redaction_unmappable"));
        }
    }
}
