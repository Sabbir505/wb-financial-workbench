//! F7: local-first review audit trail. Every review action is appended to a JSONL
//! log next to the app data (survives research-service outages) and can be exported
//! as a pretty JSON array.

use crate::model::{ReviewReceipt, ReviewRequest, Verdict};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Wall-clock time of the WB-side action (ISO, local time).
    pub ts: String,
    pub task_id: String,
    pub object_name: String,
    pub period: String,
    /// 通过 / 驳回 / 修改后通过
    pub verdict: String,
    pub reviewer: String,
    pub comment: String,
    /// True when the reviewer submitted revised sections.
    pub revised: bool,
    /// Receipt returned by the research service (I3), kept verbatim.
    pub receipt: serde_json::Value,
    pub review_id: String,
    pub status_after: String,
    pub trace_id: String,
}

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn at(path: impl Into<PathBuf>) -> AuditLog {
        AuditLog { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, record: &AuditRecord) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(record).expect("audit record serializes");
        line.push('\n');
        f.write_all(line.as_bytes())
    }

    pub fn list(&self) -> Vec<AuditRecord> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Local-first review record: built from the request + the I3 receipt,
    /// appended before anything else happens (survives service outages).
    pub fn record_review(
        &self,
        req: &ReviewRequest,
        object_name: &str,
        period: &str,
        receipt: &ReviewReceipt,
    ) -> std::io::Result<AuditRecord> {
        let record = AuditRecord {
            ts: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            task_id: req.task_id.clone(),
            object_name: object_name.to_string(),
            period: period.to_string(),
            verdict: req.verdict.as_str().to_string(),
            reviewer: req.reviewer.clone(),
            comment: req.comment.clone().unwrap_or_default(),
            revised: req.verdict == Verdict::ApproveAsRevised,
            receipt: serde_json::to_value(receipt).unwrap_or_default(),
            review_id: receipt.review_id.clone(),
            status_after: receipt.status.clone(),
            trace_id: receipt.trace_id.clone().unwrap_or_default(),
        };
        self.append(&record)?;
        Ok(record)
    }

    /// Export as a pretty-printed JSON array (to any writable path).
    pub fn export_json(&self, dest: &Path) -> std::io::Result<usize> {
        let records = self.list();
        let json = serde_json::to_string_pretty(&records).expect("audit list serializes");
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, json)?;
        Ok(records.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(task: &str, verdict: &str) -> AuditRecord {
        AuditRecord {
            ts: "2026-11-02T09:30:00".into(),
            task_id: task.into(),
            object_name: "斯迪克".into(),
            period: "2026Q4".into(),
            verdict: verdict.into(),
            reviewer: "张研".into(),
            comment: "ok".into(),
            revised: verdict == "修改后通过",
            receipt: serde_json::json!({"ok": true, "review_id": "RV-20261102-0001"}),
            review_id: "RV-20261102-0001".into(),
            status_after: "SUCCEEDED".into(),
            trace_id: "PR-EV-1".into(),
        }
    }

    #[test]
    fn append_list_export_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let log = AuditLog::at(tmp.path().join("reviews.jsonl"));
        assert!(log.list().is_empty());
        log.append(&record("T1", "通过")).unwrap();
        log.append(&record("T2", "修改后通过")).unwrap();
        let list = log.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].verdict, "修改后通过");
        assert!(list[1].revised);

        let dest = tmp.path().join("export").join("reviews.json");
        let n = log.export_json(&dest).unwrap();
        assert_eq!(n, 2);
        let text = std::fs::read_to_string(&dest).unwrap();
        let parsed: Vec<AuditRecord> = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].task_id, "T1");
    }
}
