//! System health and hardware detection.
//!
//! Per `rules.md` §4.2: no SQL here — delegates to the pool for a
//! connectivity check and uses `sysinfo` for hardware detection.

use serde::Serialize;
use sqlx::SqlitePool;
use sysinfo::System;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    pub db_ok: bool,
    pub os_name: String,
    pub os_version: String,
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
    pub cpu_count: usize,
}

pub async fn check(pool: &SqlitePool) -> AppResult<HealthStatus> {
    let db_ok = sqlx::query("SELECT 1").execute(pool).await.is_ok();

    let mut sys = System::new();
    sys.refresh_memory();

    Ok(HealthStatus {
        db_ok,
        os_name: System::name().unwrap_or_else(|| "unknown".into()),
        os_version: System::os_version().unwrap_or_else(|| "unknown".into()),
        total_memory_mb: sys.total_memory() / (1024 * 1024),
        available_memory_mb: sys.available_memory() / (1024 * 1024),
        cpu_count: sys.cpus().len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_serializes_to_camel_case() {
        let status = HealthStatus {
            db_ok: true,
            os_name: "Test".into(),
            os_version: "1.0".into(),
            total_memory_mb: 16384,
            available_memory_mb: 8192,
            cpu_count: 8,
        };
        let json = serde_json::to_value(&status).expect("serialize");
        assert_eq!(json["dbOk"], true);
        assert_eq!(json["totalMemoryMb"], 16384);
        assert_eq!(json["cpuCount"], 8);
    }
}
