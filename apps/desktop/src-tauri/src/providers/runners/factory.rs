//! Per-language [`TestRunner`] selection (`plan/SANDBOX_PYTHON_RUNNER.md`
//! §4.2). Mirrors `providers/factory.rs` for LLM/embedding providers:
//! callers (the `run_test_sandbox` command via `SandboxDeps`) depend on
//! this function, never on a concrete runner type, so adding a language
//! is one `docker_<lang>.rs` + one match arm.

use std::sync::Arc;

use super::docker_js::DockerJsRunner;
use super::docker_py::DockerPyRunner;
use super::{RunnerLanguage, TestRunner};

/// Select the Docker runner for the language detected from the artifact's
/// `files[]` (see `sandbox_service::build_run_input`).
#[must_use]
pub fn runner_for(language: RunnerLanguage) -> Arc<dyn TestRunner> {
    match language {
        RunnerLanguage::JavaScript | RunnerLanguage::TypeScript => Arc::new(DockerJsRunner::new()),
        RunnerLanguage::Python => Arc::new(DockerPyRunner::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_maps_languages_to_their_runners() {
        assert_eq!(runner_for(RunnerLanguage::JavaScript).name(), "docker-js");
        assert_eq!(runner_for(RunnerLanguage::TypeScript).name(), "docker-js");
        assert_eq!(runner_for(RunnerLanguage::Python).name(), "docker-py");
    }
}
