//! WB core data model — shapes mirror the KG-Palantir v3 contract and the
//! `vendor/api_mock` real-run samples. Amounts are in 亿元 (0.669378 = 6693.78万元),
//! percentages are plain numbers (47.86 = 47.86%).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The 13 frontmatter fields of the I2 brief file contract.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Frontmatter {
    pub task_id: String,
    pub trace_id: String,
    pub event_ref: String,
    pub object_id: String,
    pub object_version: String,
    pub period: String,
    pub workflow_version: String,
    pub status: String,
    #[serde(default)]
    pub review_comment: String,
    #[serde(default)]
    pub reviewer: String,
    pub created_at: String,
    pub data_cutoff: String,
    pub source_version: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub from: Option<String>,
    pub to: String,
    #[serde(default)]
    pub note: String,
}

/// Queue row (`GET /analysis/history` / MCP `list_pending`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_id: String,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub object_name: String,
    #[serde(default)]
    pub period: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub fail_reason: Option<String>,
}

/// `sections.calculation` accepts either a JSON object or a list of `key = value`
/// lines (the vault brief renders it as `key = value` lines).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalcValues(pub BTreeMap<String, Value>);

impl Serialize for CalcValues {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for CalcValues {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(CalcValues(parse_calc_value(&v)))
    }
}

fn parse_calc_value(v: &Value) -> BTreeMap<String, Value> {
    match v {
        Value::Object(m) => m
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        Value::Array(lines) => {
            let mut out = BTreeMap::new();
            for line in lines.iter().filter_map(|l| l.as_str()) {
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().to_string();
                    let raw = v.trim();
                    let val = match raw.parse::<f64>() {
                        Ok(n) => Value::from(n),
                        Err(_) => match raw {
                            "true" => Value::Bool(true),
                            "false" => Value::Bool(false),
                            other => Value::String(other.to_string()),
                        },
                    };
                    out.insert(key, val);
                }
            }
            out
        }
        _ => BTreeMap::new(),
    }
}

/// The four fixed brief sections: FACT / CALCULATION / INFERENCE / CHECK.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BriefSections {
    #[serde(default)]
    pub fact: FactList,
    #[serde(default)]
    pub calculation: CalcValues,
    #[serde(default)]
    pub inference: String,
    #[serde(default)]
    pub check: String,
}

/// FACT accepts either a list of strings or one paragraph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FactList(pub Vec<String>);

impl Serialize for FactList {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for FactList {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match v {
            Value::Array(a) => Ok(FactList(
                a.into_iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect(),
            )),
            Value::String(s) => Ok(FactList(vec![s])),
            _ => Ok(FactList(vec![])),
        }
    }
}

/// Full task payload (`GET /analysis/{task_id}` / MCP `get_task`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaskDetail {
    pub task_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub object_name: String,
    #[serde(default)]
    pub period: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub workflow_version: String,
    #[serde(default)]
    pub data_cutoff: String,
    #[serde(default)]
    pub source_version: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub sections: BriefSections,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub brief_path: Option<String>,
    #[serde(default)]
    pub brief_version: Option<u64>,
    #[serde(default)]
    pub fail_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Announcement {
    pub event_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub event_date: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub pct_change: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Company {
    pub object_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ticker: String,
    #[serde(default)]
    pub exchange: String,
    #[serde(default)]
    pub sector: String,
}

/// Frozen context snapshot (MCP `get_context`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextBundle {
    pub task_id: String,
    #[serde(default)]
    pub data_cutoff: String,
    #[serde(default)]
    pub source_version: String,
    #[serde(default)]
    pub announcement: Announcement,
    #[serde(default)]
    pub company: Company,
    #[serde(default)]
    pub metrics: CalcValues,
}

/// Evidence pack (`GET /audit/{task_id}/evidence`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Evidence {
    pub event_id: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub fetched_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ValidationInfo {
    #[serde(default)]
    pub tolerance_pct: f64,
    #[serde(default)]
    pub pp_tolerance: f64,
    #[serde(default)]
    pub checked_at: String,
}

/// Pipeline methodology (`GET /audit/{task_id}/methodology`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Methodology {
    pub task_id: String,
    #[serde(default)]
    pub workflow_version: String,
    #[serde(default)]
    pub data_cutoff: String,
    #[serde(default)]
    pub source_version: String,
    #[serde(default)]
    pub pipeline: Vec<String>,
    #[serde(default)]
    pub validation: ValidationInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stock {
    pub object_id: String,
    pub name: String,
    pub ticker: String,
    #[serde(default)]
    pub exchange: String,
    #[serde(default)]
    pub sector: String,
    #[serde(default)]
    pub listed: bool,
}

// ---------------------------------------------------------------------------
// Review write path (I3 submit_review / I7-shape request body)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// 通过
    #[serde(rename = "通过")]
    Approve,
    /// 驳回
    #[serde(rename = "驳回")]
    Reject,
    /// 修改后通过
    #[serde(rename = "修改后通过")]
    ApproveAsRevised,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Approve => "通过",
            Verdict::Reject => "驳回",
            Verdict::ApproveAsRevised => "修改后通过",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewRequest {
    pub task_id: String,
    pub verdict: Verdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub reviewer: String,
    pub idempotency_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_sections: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewReceipt {
    pub ok: bool,
    pub review_id: String,
    pub status: String,
    #[serde(default)]
    pub brief_version: Option<u64>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConflictBody {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub current_status: String,
}
