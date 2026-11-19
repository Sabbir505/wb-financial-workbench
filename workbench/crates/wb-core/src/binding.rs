//! Value + binding traceability (F4 layer 1b).
//!
//! The value check answers "does this number exist in the evidence?" The
//! binding check answers the question the value check cannot: "is it the same
//! account, period, scope and unit?" A claim can pass the value check and still
//! be wrong, because `2.15` exists in the evidence under a different metric, a
//! different quarter, or a different consolidation scope.
//!
//! Everything in this module is deterministic. There is no model call and no
//! fuzzy semantic similarity. The alias table and the period/scope parsers are
//! the only sources of interpretation, and they are data in this file.
//!
//! ## Verdicts
//!
//! * `Supported`  value matches and every comparable binding dimension matches.
//! * `ValueOnly`  value matches but no metric/period/scope cue is available to
//!   bind against. This is deliberately not a flag: abstaining is safer than
//!   guessing, and it keeps the false-positive rate honest.
//! * `Mismatch`   value matches but a comparable dimension differs. This is the
//!   "right number, wrong account" case.
//! * `Unsupported` no value match at all. This is the red flag the existing
//!   guardrail already produces.
//!
//! Binding mismatches never set `Guardrail::ok()` to false. The supervisor's
//! acceptance criterion is about invented numbers, and the binding result is a
//! second, separately reported verdict.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::guardrail::{scan_numbers, within_tolerance, GuardrailConfig, RawNumber};
use crate::model::{Evidence, EvidenceFactDto};

// ---------------------------------------------------------------------------
// Units, periods, scopes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Money. `value` is normalised to yuan.
    Amount,
    /// Percentage points. `value` is the number as printed (47.86 means 47.86%).
    Percent,
    /// No unit cue. Matching falls back to the source's native scale.
    Unknown,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Amount => "金额",
            Unit::Percent => "百分比",
            Unit::Unknown => "未标注单位",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountScale {
    Yuan,
    Wan,
    Yi,
    Unknown,
}

impl AmountScale {
    pub fn factor(self) -> f64 {
        match self {
            AmountScale::Yuan => 1.0,
            AmountScale::Wan => 1.0e4,
            AmountScale::Yi => 1.0e8,
            AmountScale::Unknown => 1.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AmountScale::Yuan => "元",
            AmountScale::Wan => "万元",
            AmountScale::Yi => "亿元",
            AmountScale::Unknown => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Consolidated,
    Parent,
    Deducted,
    Total,
    Unknown,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Consolidated => "合并口径",
            Scope::Parent => "母公司/归母口径",
            Scope::Deducted => "扣非口径",
            Scope::Total => "合计口径",
            Scope::Unknown => "未标注口径",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodSpan {
    Q1,
    Q2,
    Q3,
    Q4,
    H1,
    H2,
    /// Cumulative nine months, i.e. 前三季度.
    M9,
    Fy,
    Ttm,
    Unknown,
}

impl PeriodSpan {
    pub fn as_str(self) -> &'static str {
        match self {
            PeriodSpan::Q1 => "Q1",
            PeriodSpan::Q2 => "Q2",
            PeriodSpan::Q3 => "Q3",
            PeriodSpan::Q4 => "Q4",
            PeriodSpan::H1 => "上半年",
            PeriodSpan::H2 => "下半年",
            PeriodSpan::M9 => "前三季度",
            PeriodSpan::Fy => "全年",
            PeriodSpan::Ttm => "TTM",
            PeriodSpan::Unknown => "未标注期间",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub year: Option<i32>,
    pub span: PeriodSpan,
}

impl Period {
    pub const fn unknown() -> Self {
        Period {
            year: None,
            span: PeriodSpan::Unknown,
        }
    }

    pub fn is_unknown(self) -> bool {
        self.year.is_none() && self.span == PeriodSpan::Unknown
    }

    /// Two periods are comparable when at least one component is known on both
    /// sides. A year-only fact can still be checked against a year+span claim.
    pub fn is_comparable(self, other: Period) -> bool {
        (self.span != PeriodSpan::Unknown && other.span != PeriodSpan::Unknown)
            || (self.year.is_some() && other.year.is_some())
    }

    pub fn compatible(self, other: Period) -> bool {
        if let (Some(a), Some(b)) = (self.year, other.year) {
            if a != b {
                return false;
            }
        }
        if self.span != PeriodSpan::Unknown
            && other.span != PeriodSpan::Unknown
            && self.span != other.span
        {
            return false;
        }
        true
    }

    pub fn label(self) -> String {
        match (self.year, self.span) {
            (Some(y), PeriodSpan::Unknown) => format!("{y}年"),
            (Some(y), s) => format!("{y}{}", s.as_str()),
            (None, PeriodSpan::Unknown) => "未标注期间".to_string(),
            (None, s) => s.as_str().to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    None,
    YoY,
    QoQ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactSource {
    Metric,
    Calc,
    Excerpt,
    Legacy,
}

impl FactSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FactSource::Metric => "metric",
            FactSource::Calc => "calc",
            FactSource::Excerpt => "excerpt",
            FactSource::Legacy => "legacy",
        }
    }
}

// ---------------------------------------------------------------------------
// Facts and claim cues
// ---------------------------------------------------------------------------

/// One evidence fact with enough structure to bind a claim number to it.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceFact {
    /// Stable upstream metric id, e.g. `revenue` or `net_profit_deducted`.
    pub metric_id: Option<String>,
    /// For a growth rate, the metric whose growth it describes.
    pub base_metric_id: Option<String>,
    /// Normalised value: yuan for `Amount`, percentage points for `Percent`.
    pub value: f64,
    /// The value as printed by the source, in `scale`.
    pub native: f64,
    pub unit: Unit,
    pub scale: AmountScale,
    pub period: Period,
    pub scope: Scope,
    pub source: FactSource,
    pub label: String,
    pub locator: String,
}

/// One number in the model-written text, with the cues needed to bind it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaimCue {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    pub native: f64,
    pub unit: Unit,
    pub scale: AmountScale,
    pub metric_id: Option<String>,
    pub base_metric_id: Option<String>,
    pub period: Period,
    pub comparison: Comparison,
    pub scope: Scope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingDimension {
    Metric,
    BaseMetric,
    Period,
    Scope,
    Unit,
}

impl BindingDimension {
    pub fn as_str(self) -> &'static str {
        match self {
            BindingDimension::Metric => "口径",
            BindingDimension::BaseMetric => "基数指标",
            BindingDimension::Period => "期间",
            BindingDimension::Scope => "合并/归母口径",
            BindingDimension::Unit => "单位",
        }
    }

    fn severity(self) -> u8 {
        match self {
            BindingDimension::Metric => 0,
            BindingDimension::BaseMetric => 1,
            BindingDimension::Period => 2,
            BindingDimension::Scope => 3,
            BindingDimension::Unit => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingVerdict {
    Supported,
    ValueOnly,
    Mismatch {
        dimension: BindingDimension,
        /// What the claim says.
        expected: String,
        /// What the evidence says.
        found: String,
    },
    Unsupported,
}

/// One numeric token with its binding verdict. `Guardrail.bindings` has one
/// entry per `Guardrail.spans` entry, in the same order.
#[derive(Clone, Debug, PartialEq)]
pub struct BindingSpan {
    pub start: usize,
    pub end: usize,
    pub raw: String,
    pub value: f64,
    pub verdict: BindingVerdict,
    pub fact: Option<EvidenceFact>,
    pub hint: String,
}

// ---------------------------------------------------------------------------
// Controlled vocabulary
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct MetricAlias {
    pub id: &'static str,
    pub text: &'static str,
    pub scope: Scope,
}

/// The alias table is intentionally small and conservative. Every entry maps a
/// Chinese financial phrase to one of the upstream metric keys. Adding an entry
/// is a data change, reviewable in one diff.
pub const ALIASES: &[MetricAlias] = &[
    // Revenue
    MetricAlias { id: "revenue", text: "营业总收入", scope: Scope::Unknown },
    MetricAlias { id: "revenue", text: "营业收入", scope: Scope::Unknown },
    MetricAlias { id: "revenue", text: "营收", scope: Scope::Unknown },
    // Net profit, attributable to the parent
    MetricAlias { id: "net_profit", text: "归属于上市公司股东的净利润", scope: Scope::Parent },
    MetricAlias { id: "net_profit", text: "归属于母公司股东的净利润", scope: Scope::Parent },
    MetricAlias { id: "net_profit", text: "归母净利润", scope: Scope::Parent },
    MetricAlias { id: "net_profit", text: "归母净利", scope: Scope::Parent },
    MetricAlias { id: "net_profit", text: "净利润", scope: Scope::Unknown },
    // Net profit, excluding non-recurring items
    MetricAlias { id: "net_profit_deducted", text: "扣除非经常性损益后的净利润", scope: Scope::Deducted },
    MetricAlias { id: "net_profit_deducted", text: "扣除非经常性损益的净利润", scope: Scope::Deducted },
    MetricAlias { id: "net_profit_deducted", text: "扣非归母净利润", scope: Scope::Deducted },
    MetricAlias { id: "net_profit_deducted", text: "扣非净利润", scope: Scope::Deducted },
    MetricAlias { id: "net_profit_deducted", text: "扣非净利", scope: Scope::Deducted },
    MetricAlias { id: "net_profit_deducted", text: "扣非", scope: Scope::Deducted },
    // Cash flow
    MetricAlias { id: "ocf", text: "经营活动产生的现金流量净额", scope: Scope::Consolidated },
    MetricAlias { id: "ocf", text: "经营活动现金流量净额", scope: Scope::Consolidated },
    MetricAlias { id: "ocf", text: "经营现金流", scope: Scope::Consolidated },
    // Growth rates
    MetricAlias { id: "yoy_first", text: "同比增长", scope: Scope::Unknown },
    MetricAlias { id: "yoy_first", text: "同比下降", scope: Scope::Unknown },
    MetricAlias { id: "yoy_first", text: "同比", scope: Scope::Unknown },
    MetricAlias { id: "pct_change_event", text: "环比增长", scope: Scope::Unknown },
    MetricAlias { id: "pct_change_event", text: "环比下降", scope: Scope::Unknown },
    MetricAlias { id: "pct_change_event", text: "环比", scope: Scope::Unknown },
    // Margins
    MetricAlias { id: "gross_margin_pct", text: "销售毛利率", scope: Scope::Unknown },
    MetricAlias { id: "gross_margin_pct", text: "毛利率", scope: Scope::Unknown },
    MetricAlias { id: "net_margin_pct", text: "销售净利率", scope: Scope::Unknown },
    MetricAlias { id: "net_margin_pct", text: "净利率", scope: Scope::Unknown },
    // Other common accounts
    MetricAlias { id: "operating_profit", text: "营业利润", scope: Scope::Unknown },
    MetricAlias { id: "total_profit", text: "利润总额", scope: Scope::Unknown },
    MetricAlias { id: "eps", text: "基本每股收益", scope: Scope::Unknown },
    MetricAlias { id: "eps", text: "每股收益", scope: Scope::Unknown },
    MetricAlias { id: "total_assets", text: "总资产", scope: Scope::Unknown },
    MetricAlias { id: "net_assets", text: "净资产", scope: Scope::Unknown },
    MetricAlias { id: "debt_ratio_pct", text: "资产负债率", scope: Scope::Unknown },
    MetricAlias { id: "construction_in_progress", text: "在建工程", scope: Scope::Unknown },
    MetricAlias { id: "inventory", text: "存货", scope: Scope::Unknown },
    MetricAlias { id: "accounts_receivable", text: "应收账款", scope: Scope::Unknown },
];

fn aliases_by_length() -> &'static [MetricAlias] {
    static SORTED: OnceLock<Vec<MetricAlias>> = OnceLock::new();
    SORTED.get_or_init(|| {
        let mut out = ALIASES.to_vec();
        out.sort_by(|a, b| {
            b.text
                .chars()
                .count()
                .cmp(&a.text.chars().count())
                .then_with(|| a.text.cmp(b.text))
        });
        out
    })
}

pub fn metric_name(id: &str) -> &'static str {
    match id {
        "revenue" => "营业收入",
        "net_profit" => "归母净利润",
        "net_profit_deducted" => "扣非净利润",
        "ocf" => "经营活动现金流量净额",
        "yoy_first" => "同比增长",
        "pct_change_event" => "环比增长",
        "gross_margin_pct" => "毛利率",
        "net_margin_pct" => "净利率",
        "operating_profit" => "营业利润",
        "total_profit" => "利润总额",
        "eps" => "每股收益",
        "total_assets" => "总资产",
        "net_assets" => "净资产",
        "debt_ratio_pct" => "资产负债率",
        "construction_in_progress" => "在建工程",
        "inventory" => "存货",
        "accounts_receivable" => "应收账款",
        _ => "未知口径",
    }
}

// ---------------------------------------------------------------------------
// Period, scope and unit parsers
// ---------------------------------------------------------------------------

fn period_regexes() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        vec![
            // 2026Q3 / 2026年Q3
            Regex::new(r"(?i)(?P<y>\d{4})\s*年?\s*Q\s*(?P<q>[1-4])").unwrap(),
            // 2026年第三季度 / 2026年第3季度
            Regex::new(r"(?P<y>\d{4})\s*年\s*第?\s*(?P<q>[一二三四1-4])\s*季度").unwrap(),
            // 2026年度: the 年 is shared by the year and the span, so this
            // needs its own pattern before the general one below.
            Regex::new(r"(?P<y>\d{4})\s*(?P<s>年度)").unwrap(),
            // 2026年前三季度 / 2026年上半年 / 2026年全年
            Regex::new(
                r"(?P<y>\d{4})\s*年\s*(?P<s>前三季度|前三季|上半年|半年度|全年)",
            )
            .unwrap(),
            // M9 / H1 / FY in a structured period string
            Regex::new(r"(?i)(?P<y>\d{4})\s*M\s*9").unwrap(),
            Regex::new(r"(?i)(?P<y>\d{4})\s*H\s*1").unwrap(),
            Regex::new(r"(?i)(?P<y>\d{4})\s*(?:FY|FYE)").unwrap(),
            // A bare Q3, usually in "Q3单季"
            Regex::new(r"(?i)(?P<q>Q\s*[1-4])").unwrap(),
            // 前三季度 / 上半年 / 第三季度 / 单季, no year
            Regex::new(r"(?P<s>前三季度|前三季|上半年|半年度|第三季度|三季度|单季|单季度)")
                .unwrap(),
            // A bare year, used only as a year hint.
            Regex::new(r"(?P<y>\d{4})\s*年").unwrap(),
        ]
    })
}

fn span_from_cn(s: &str) -> PeriodSpan {
    match s {
        "前三季度" | "前三季" => PeriodSpan::M9,
        "上半年" | "半年度" => PeriodSpan::H1,
        "年度" | "全年" => PeriodSpan::Fy,
        "第三季度" | "三季度" | "单季" | "单季度" => PeriodSpan::Q3,
        _ => PeriodSpan::Unknown,
    }
}

fn span_from_quarter_token(s: &str) -> PeriodSpan {
    match s.trim().to_ascii_uppercase().as_str() {
        "1" | "一" => PeriodSpan::Q1,
        "2" | "二" => PeriodSpan::Q2,
        "3" | "三" => PeriodSpan::Q3,
        "4" | "四" => PeriodSpan::Q4,
        "Q1" => PeriodSpan::Q1,
        "Q2" => PeriodSpan::Q2,
        "Q3" => PeriodSpan::Q3,
        "Q4" => PeriodSpan::Q4,
        _ => PeriodSpan::Unknown,
    }
}

/// Parse the first explicit period expression in `text`.
pub fn parse_period(text: &str) -> Option<Period> {
    for re in period_regexes() {
        if let Some(caps) = re.captures(text) {
            let year = caps
                .name("y")
                .and_then(|m| m.as_str().parse::<i32>().ok());
            let span = if let Some(q) = caps.name("q") {
                span_from_quarter_token(q.as_str())
            } else if let Some(s) = caps.name("s") {
                span_from_cn(s.as_str())
            } else if re.as_str().contains("M") {
                PeriodSpan::M9
            } else if re.as_str().contains("H") {
                PeriodSpan::H1
            } else if re.as_str().contains("FY") {
                PeriodSpan::Fy
            } else {
                PeriodSpan::Unknown
            };
            if year.is_some() || span != PeriodSpan::Unknown {
                return Some(Period { year, span });
            }
        }
    }
    None
}

fn comparison_in(text: &str) -> Comparison {
    if text.contains("环比") {
        Comparison::QoQ
    } else if text.contains("同比") {
        Comparison::YoY
    } else {
        Comparison::None
    }
}

fn scope_from_text(text: &str) -> Scope {
    let mut found = Scope::Unknown;
    let mark = |current: Scope, next: Scope| -> Scope {
        if current == Scope::Unknown || current == next {
            next
        } else {
            Scope::Unknown
        }
    };
    if text.contains("母公司") || text.contains("归母") {
        found = mark(found, Scope::Parent);
    }
    if text.contains("合并") {
        found = mark(found, Scope::Consolidated);
    }
    if text.contains("扣非") || text.contains("扣除非经常性损益") {
        found = mark(found, Scope::Deducted);
    }
    if text.contains("合计") || text.contains("全部") {
        found = mark(found, Scope::Total);
    }
    found
}

fn combine_scope(alias: Scope, text: Scope) -> Scope {
    match (alias, text) {
        (Scope::Unknown, t) => t,
        (a, Scope::Unknown) => a,
        // The alias is more specific than a generic word in the sentence.
        (a, _) => a,
    }
}

fn unit_from_str(s: &str) -> (Unit, AmountScale) {
    let s = s.trim();
    if s.contains('%') || s.contains('％') || s.contains("百分点") {
        (Unit::Percent, AmountScale::Unknown)
    } else if s.contains("亿元") {
        (Unit::Amount, AmountScale::Yi)
    } else if s.contains("万元") {
        (Unit::Amount, AmountScale::Wan)
    } else if s.contains('元') {
        (Unit::Amount, AmountScale::Yuan)
    } else {
        (Unit::Unknown, AmountScale::Unknown)
    }
}

fn unit_from_suffix(text: &str, end: usize, is_pct: bool) -> (Unit, AmountScale) {
    if is_pct {
        return (Unit::Percent, AmountScale::Unknown);
    }
    let tail = text[end..].trim_start();
    if tail.starts_with('%') || tail.starts_with('％') || tail.starts_with("个百分点") {
        (Unit::Percent, AmountScale::Unknown)
    } else if tail.starts_with("亿元") {
        (Unit::Amount, AmountScale::Yi)
    } else if tail.starts_with("万元") {
        (Unit::Amount, AmountScale::Wan)
    } else if tail.starts_with('元') {
        (Unit::Amount, AmountScale::Yuan)
    } else if tail.starts_with('亿') {
        (Unit::Amount, AmountScale::Yi)
    } else if tail.starts_with('万') {
        (Unit::Amount, AmountScale::Wan)
    } else {
        (Unit::Unknown, AmountScale::Unknown)
    }
}

fn normalize(value: f64, unit: Unit, scale: AmountScale) -> f64 {
    match unit {
        Unit::Amount => value * scale.factor(),
        Unit::Percent | Unit::Unknown => value,
    }
}

fn scope_from_dto(s: &str) -> Scope {
    match s.trim().to_ascii_lowercase().as_str() {
        "consolidated" | "合并" | "合并口径" => Scope::Consolidated,
        "parent" | "母公司" | "归母" | "归母口径" => Scope::Parent,
        "deducted" | "扣非" | "扣非口径" => Scope::Deducted,
        "total" | "合计" | "合计口径" => Scope::Total,
        _ => Scope::Unknown,
    }
}

fn unit_for_metric(key: &str, default_unit: &str) -> (Unit, AmountScale) {
    if key.ends_with("_pct") || key.starts_with("yoy") || key.starts_with("pct_change") {
        (Unit::Percent, AmountScale::Unknown)
    } else {
        unit_from_str(default_unit)
    }
}

fn scope_for_metric(key: &str) -> Scope {
    match key {
        "net_profit_deducted" => Scope::Deducted,
        "ocf" => Scope::Consolidated,
        // `net_profit` is ambiguous upstream: it may be total or attributable.
        // Do not guess. The evidence pack can supply an explicit scope.
        _ => Scope::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Public builders
// ---------------------------------------------------------------------------

/// Build evidence facts from a metric map. `default_unit` is the unit of the
/// amount metrics (the fixtures use 亿元); percentage keys are detected from
/// the key name, not from that unit.
pub fn facts_from_metrics(
    metrics: &BTreeMap<String, Value>,
    default_unit: &str,
    period: Period,
    source: FactSource,
) -> Vec<EvidenceFact> {
    let mut out = Vec::new();
    for (key, value) in metrics {
        let Some(n) = value.as_f64() else {
            continue;
        };
        if !n.is_finite() {
            continue;
        }
        let (unit, scale) = unit_for_metric(key, default_unit);
        out.push(EvidenceFact {
            metric_id: Some(key.clone()),
            base_metric_id: None,
            value: normalize(n, unit, scale),
            native: n,
            unit,
            scale,
            period,
            scope: scope_for_metric(key),
            source,
            label: metric_name(key).to_string(),
            locator: source.as_str().to_string(),
        });
    }
    out
}

/// Build facts from a free-text evidence source (announcement summary, audit
/// excerpt, CALC prose). The same cue extractor used for claims is used here,
/// so a value and its label are read the same way on both sides.
pub fn facts_from_text(
    text: &str,
    default_period: Period,
    source: FactSource,
    label: &str,
) -> Vec<EvidenceFact> {
    extract_cues(text, default_period)
        .into_iter()
        .map(|cue| EvidenceFact {
            metric_id: cue.metric_id.clone(),
            base_metric_id: cue.base_metric_id.clone(),
            value: cue.value,
            native: cue.native,
            unit: cue.unit,
            scale: cue.scale,
            period: cue.period,
            scope: cue.scope,
            source,
            label: cue
                .metric_id
                .as_deref()
                .map(metric_name)
                .unwrap_or(label)
                .to_string(),
            locator: label.to_string(),
        })
        .collect()
}

/// The sources one task can contribute to the fact set. All fields are
/// optional so the same builder works online (context + evidence + CALC) and
/// offline (vault brief only).
#[derive(Default)]
pub struct FactSources<'a> {
    /// Deterministic metrics from the context bundle.
    pub metrics: Option<&'a BTreeMap<String, Value>>,
    /// Unit of the amount metrics. The fixtures use 亿元.
    pub metric_unit: &'a str,
    /// Announcement summary (free text).
    pub summary: Option<&'a str>,
    /// Evidence pack, including its optional structured `facts`.
    pub evidence: Option<&'a Evidence>,
    /// The brief's own CALCULATION values.
    pub calc: Option<&'a BTreeMap<String, f64>>,
    /// FACT section body, used only on the degraded offline path.
    pub fact_text: Option<&'a str>,
}

/// Build the complete fact set from all available sources. This is the single
/// entry point shared by the desktop app and the e2e suite, so the two can
/// never drift apart.
pub fn facts_from_sources(sources: FactSources<'_>) -> Vec<EvidenceFact> {
    let mut facts = Vec::new();
    if let Some(metrics) = sources.metrics {
        facts.extend(facts_from_metrics(
            metrics,
            sources.metric_unit,
            Period::unknown(),
            FactSource::Metric,
        ));
    }
    if let Some(summary) = sources.summary {
        facts.extend(facts_from_text(
            summary,
            Period::unknown(),
            FactSource::Excerpt,
            "公告摘要",
        ));
    }
    if let Some(ev) = sources.evidence {
        facts.extend(facts_from_dto(&ev.facts, FactSource::Excerpt));
        facts.extend(facts_from_text(
            &ev.excerpt,
            Period::unknown(),
            FactSource::Excerpt,
            &ev.locator,
        ));
    }
    if let Some(calc) = sources.calc {
        let mut map = BTreeMap::new();
        for (k, v) in calc {
            map.insert(k.clone(), Value::from(*v));
        }
        facts.extend(facts_from_metrics(
            &map,
            sources.metric_unit,
            Period::unknown(),
            FactSource::Calc,
        ));
    }
    if let Some(text) = sources.fact_text {
        facts.extend(facts_from_text(
            text,
            Period::unknown(),
            FactSource::Excerpt,
            "FACT",
        ));
    }
    facts
}

/// Build facts from the structured evidence-pack extension. This is the path
/// the professor's ontology should use: the service owns the metric id, unit,
/// period and scope, and the workbench only checks them.
pub fn facts_from_dto(facts: &[EvidenceFactDto], source: FactSource) -> Vec<EvidenceFact> {
    facts
        .iter()
        .filter_map(|f| {
            if !f.value.is_finite() {
                return None;
            }
            let (unit, scale) = if f.unit.trim().is_empty() {
                unit_for_metric(&f.metric_id, "")
            } else {
                unit_from_str(&f.unit)
            };
            let period = if f.period.trim().is_empty() {
                Period::unknown()
            } else {
                parse_period(&f.period).unwrap_or(Period::unknown())
            };
            Some(EvidenceFact {
                metric_id: if f.metric_id.trim().is_empty() {
                    None
                } else {
                    Some(f.metric_id.clone())
                },
                base_metric_id: if f.base_metric_id.trim().is_empty() {
                    None
                } else {
                    Some(f.base_metric_id.clone())
                },
                value: normalize(f.value, unit, scale),
                native: f.value,
                unit,
                scale,
                period,
                scope: scope_from_dto(&f.scope),
                source,
                label: if f.metric_name.trim().is_empty() {
                    metric_name(&f.metric_id).to_string()
                } else {
                    f.metric_name.clone()
                },
                locator: f.locator.clone(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Claim extraction
// ---------------------------------------------------------------------------

pub fn extract_cues(text: &str, default_period: Period) -> Vec<ClaimCue> {
    scan_numbers(text)
        .into_iter()
        .map(|raw| cue_for(text, &raw, default_period))
        .collect()
}

pub(crate) fn cue_for(text: &str, raw: &RawNumber, default_period: Period) -> ClaimCue {
    let (clause_start, clause_end) = clause_bounds(text, raw.start, raw.end);
    let clause = &text[clause_start..clause_end];
    let rel_start = raw.start - clause_start;
    let rel_end = raw.end - clause_start;
    let nearest = nearest_alias(clause, rel_start, rel_end);
    let (unit, scale) = unit_from_suffix(text, raw.end, raw.is_pct);
    let period = parse_period(clause).unwrap_or(default_period);
    let comparison = comparison_in(clause);
    let growth = unit == Unit::Percent
        && (comparison != Comparison::None
            || clause.contains("增长")
            || clause.contains("增速")
            || clause.contains("下降")
            || clause.contains("减少")
            || preceded_by_sign(text, raw.start, &raw.raw));
    let (metric_id, base_metric_id) = if growth {
        let id = if comparison == Comparison::QoQ {
            "pct_change_event"
        } else {
            "yoy_first"
        };
        // The base metric is the account the growth describes. If the nearest
        // alias is the growth word itself ("同比增长"), the base is in another
        // clause and is not asserted here.
        let base = nearest.and_then(|a| {
            if a.id == "yoy_first" || a.id == "pct_change_event" {
                None
            } else {
                Some(a.id.to_string())
            }
        });
        (Some(id.to_string()), base)
    } else {
        (nearest.map(|a| a.id.to_string()), None)
    };
    let scope = combine_scope(
        nearest.map(|a| a.scope).unwrap_or(Scope::Unknown),
        scope_from_text(clause),
    );
    ClaimCue {
        start: raw.start,
        end: raw.end,
        raw: raw.raw.clone(),
        value: normalize(raw.value, unit, scale),
        native: raw.value,
        unit,
        scale,
        metric_id,
        base_metric_id,
        period,
        comparison,
        scope,
    }
}

fn preceded_by_sign(text: &str, start: usize, raw: &str) -> bool {
    // The number regex includes the optional sign in the match, so a leading
    // "+" or "-" is part of `raw`, not the character before `start`.
    if raw.starts_with('+') || raw.starts_with('-') {
        return true;
    }
    text[..start]
        .chars()
        .rev()
        .find(|c| !c.is_whitespace())
        .map(|c| c == '+' || c == '-')
        .unwrap_or(false)
}

const CLAUSE_BOUNDARIES: &[char] = &[
    '。', '；', ';', '，', ',', '\n', '、', '：', ':', '（', '）', '(', ')',
];

fn clause_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let clause_start = text[..start]
        .char_indices()
        .rev()
        .find(|(_, c)| CLAUSE_BOUNDARIES.contains(c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let clause_end = text[end..]
        .char_indices()
        .find(|(_, c)| CLAUSE_BOUNDARIES.contains(c))
        .map(|(i, _)| end + i)
        .unwrap_or(text.len());
    (clause_start, clause_end)
}

/// Longest non-overlapping alias match in `clause`, then the closest one to the
/// number. A 30-character window keeps a number from binding to a metric in a
/// different sentence.
fn nearest_alias(clause: &str, num_start: usize, num_end: usize) -> Option<MetricAlias> {
    let mut candidates: Vec<(usize, usize, MetricAlias)> = Vec::new();
    let mut i = 0usize;
    while i < clause.len() {
        if !clause.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let mut found = None;
        for alias in aliases_by_length() {
            if clause[i..].starts_with(alias.text) {
                found = Some(*alias);
                break;
            }
        }
        if let Some(alias) = found {
            let end = i + alias.text.len();
            candidates.push((i, end, alias));
            i = end;
        } else {
            i += clause[i..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        }
    }

    let mut best: Option<(usize, bool, MetricAlias)> = None;
    for (start, end, alias) in candidates {
        let (distance, left) = if end <= num_start {
            (clause[end..num_start].chars().count(), true)
        } else if start >= num_end {
            (clause[num_end..start].chars().count(), false)
        } else {
            (0, true)
        };
        if distance > 30 {
            continue;
        }
        let better = match best {
            None => true,
            Some((best_distance, best_left, _)) => {
                distance < best_distance || (distance == best_distance && left && !best_left)
            }
        };
        if better {
            best = Some((distance, left, alias));
        }
    }
    best.map(|(_, _, alias)| alias)
}

// ---------------------------------------------------------------------------
// Matching and verdicts
// ---------------------------------------------------------------------------

/// Value-level compatibility before binding. Percentages never borrow the
/// amount scale candidates: doing so lets `200.71%` match `0.002483` after a
/// 10,000x division, which is how a false negative happens.
pub(crate) fn value_matches(cue: &ClaimCue, fact: &EvidenceFact, cfg: &GuardrailConfig) -> bool {
    match (cue.unit, fact.unit) {
        (Unit::Percent, Unit::Percent) => within_tolerance(cue.value, fact.value, true, cfg),
        (Unit::Amount, Unit::Amount) => within_tolerance(cue.value, fact.value, false, cfg),
        (Unit::Unknown, Unit::Amount) => [cue.native, cue.native * 1e4, cue.native * 1e8]
            .iter()
            .chain([cue.native / 1e4, cue.native / 1e8].iter())
            .any(|v| within_tolerance(*v, fact.value, false, cfg)),
        (Unit::Amount, Unit::Unknown) => [cue.native, cue.native * 1e4, cue.native * 1e8]
            .iter()
            .chain([cue.native / 1e4, cue.native / 1e8].iter())
            .any(|v| within_tolerance(*v, fact.native, false, cfg)),
        (Unit::Unknown, Unit::Unknown) => {
            within_tolerance(cue.native, fact.native, false, cfg)
        }
        (Unit::Percent, Unit::Unknown) => within_tolerance(cue.value, fact.native, true, cfg),
        (Unit::Unknown, Unit::Percent) => within_tolerance(cue.native, fact.value, true, cfg),
        // An amount and a percentage are different kinds of claim.
        _ => false,
    }
}

struct CandidateScore {
    idx: usize,
    fact: EvidenceFact,
    mismatches: Vec<BindingDimension>,
    identity_comparable: u8,
    comparable: u8,
}

impl CandidateScore {
    fn better_than(&self, other: &CandidateScore) -> bool {
        // Prefer the most specific evidence first. A fact that names metric,
        // base metric and period is more informative than one that names only
        // the value, even when the specific fact exposes a mismatch.
        self.identity_comparable > other.identity_comparable
            || (self.identity_comparable == other.identity_comparable
                && self.comparable > other.comparable)
            || (self.identity_comparable == other.identity_comparable
                && self.comparable == other.comparable
                && self.mismatches.len() < other.mismatches.len())
            || (self.identity_comparable == other.identity_comparable
                && self.comparable == other.comparable
                && self.mismatches.len() == other.mismatches.len()
                && self.idx < other.idx)
    }
}

fn highest_severity(dims: &[BindingDimension]) -> BindingDimension {
    dims.iter()
        .copied()
        .min_by_key(|d| d.severity())
        .unwrap_or(BindingDimension::Metric)
}

fn claim_label(cue: &ClaimCue, dim: BindingDimension) -> String {
    match dim {
        BindingDimension::Metric => cue
            .metric_id
            .as_deref()
            .map(metric_name)
            .unwrap_or("未知口径")
            .to_string(),
        BindingDimension::BaseMetric => cue
            .base_metric_id
            .as_deref()
            .map(metric_name)
            .unwrap_or("未知基数指标")
            .to_string(),
        BindingDimension::Period => cue.period.label(),
        BindingDimension::Scope => cue.scope.as_str().to_string(),
        BindingDimension::Unit => cue.unit.as_str().to_string(),
    }
}

fn fact_label(fact: &EvidenceFact, dim: BindingDimension) -> String {
    match dim {
        BindingDimension::Metric => fact
            .metric_id
            .as_deref()
            .map(metric_name)
            .unwrap_or(fact.label.as_str())
            .to_string(),
        BindingDimension::BaseMetric => fact
            .base_metric_id
            .as_deref()
            .map(metric_name)
            .unwrap_or("未知基数指标")
            .to_string(),
        BindingDimension::Period => fact.period.label(),
        BindingDimension::Scope => fact.scope.as_str().to_string(),
        BindingDimension::Unit => fact.unit.as_str().to_string(),
    }
}

/// Pick the most specific value-compatible fact and compare the binding
/// dimensions that both sides actually specify. The matched fact is returned
/// alongside the verdict so the UI can show what it was compared against.
pub fn resolve(
    cue: &ClaimCue,
    candidates: &[(usize, &EvidenceFact)],
) -> (BindingVerdict, Option<EvidenceFact>) {
    let mut best: Option<CandidateScore> = None;
    for (idx, fact) in candidates {
        let mut mismatches = Vec::new();
        let mut identity_comparable = 0u8;
        let mut comparable = 0u8;

        if let (Some(c), Some(f)) = (&cue.metric_id, &fact.metric_id) {
            comparable += 1;
            identity_comparable += 1;
            if c != f {
                mismatches.push(BindingDimension::Metric);
            }
        }
        if let (Some(c), Some(f)) = (&cue.base_metric_id, &fact.base_metric_id) {
            comparable += 1;
            identity_comparable += 1;
            if c != f {
                mismatches.push(BindingDimension::BaseMetric);
            }
        }
        if cue.period.is_comparable(fact.period) {
            comparable += 1;
            identity_comparable += 1;
            if !cue.period.compatible(fact.period) {
                mismatches.push(BindingDimension::Period);
            }
        }
        if cue.scope != Scope::Unknown && fact.scope != Scope::Unknown {
            comparable += 1;
            identity_comparable += 1;
            if cue.scope != fact.scope {
                mismatches.push(BindingDimension::Scope);
            }
        }
        if cue.unit != Unit::Unknown && fact.unit != Unit::Unknown {
            comparable += 1;
            if cue.unit != fact.unit {
                mismatches.push(BindingDimension::Unit);
            }
        }

        let score = CandidateScore {
            idx: *idx,
            fact: (*fact).clone(),
            mismatches,
            identity_comparable,
            comparable,
        };
        if best.as_ref().map_or(true, |b| score.better_than(b)) {
            best = Some(score);
        }
    }

    let Some(best) = best else {
        return (BindingVerdict::ValueOnly, None);
    };
    let fact = Some(best.fact.clone());
    if best.mismatches.is_empty() {
        if best.identity_comparable > 0 {
            (BindingVerdict::Supported, fact)
        } else {
            (BindingVerdict::ValueOnly, fact)
        }
    } else {
        let dimension = highest_severity(&best.mismatches);
        (
            BindingVerdict::Mismatch {
                dimension,
                expected: claim_label(cue, dimension),
                found: fact_label(&best.fact, dimension),
            },
            fact,
        )
    }
}

/// Human-facing explanation for a binding verdict. Kept in Chinese to match the
/// existing guardrail hint language; the English UI can localise later without
/// touching the core.
pub fn hint_for(verdict: &BindingVerdict, cue: &ClaimCue, fact: Option<&EvidenceFact>) -> String {
    match verdict {
        BindingVerdict::Supported => {
            if let Some(f) = fact {
                let period = if f.period.is_unknown() {
                    String::new()
                } else {
                    format!("，{}", f.period.label())
                };
                format!(
                    "「{}」可回溯至 {} {}（{}{}）。",
                    cue.raw,
                    f.label,
                    format_value(f),
                    f.scope.as_str(),
                    period
                )
            } else {
                format!("「{}」可回溯至证据。", cue.raw)
            }
        }
        BindingVerdict::ValueOnly => format!(
            "「{}」数值可回溯，但句子未给出可判定的口径或期间，未做绑定判断。",
            cue.raw
        ),
        BindingVerdict::Mismatch {
            dimension,
            expected,
            found,
        } => match dimension {
            BindingDimension::Metric => format!(
                "「{}」数值可回溯，但口径不一致：句中为「{}」，证据为「{}」。",
                cue.raw, expected, found
            ),
            BindingDimension::BaseMetric => format!(
                "「{}」数值可回溯，但增长率的基数指标不一致：句中为「{}」，证据为「{}」。",
                cue.raw, expected, found
            ),
            BindingDimension::Period => format!(
                "「{}」数值可回溯，但期间不一致：句中为「{}」，证据为「{}」。",
                cue.raw, expected, found
            ),
            BindingDimension::Scope => format!(
                "「{}」数值可回溯，但合并/归母口径不一致：句中为「{}」，证据为「{}」。",
                cue.raw, expected, found
            ),
            BindingDimension::Unit => format!(
                "「{}」数值可回溯，但单位不一致：句中为「{}」，证据为「{}」。",
                cue.raw, expected, found
            ),
        },
        BindingVerdict::Unsupported => String::new(),
    }
}

fn format_value(f: &EvidenceFact) -> String {
    match f.unit {
        Unit::Percent => format!("{}%", trim_num(f.value)),
        Unit::Amount => {
            let scale = match f.scale {
                AmountScale::Yuan => "元",
                AmountScale::Wan => "万元",
                AmountScale::Yi => "亿元",
                AmountScale::Unknown => "元",
            };
            format!("{}{}", trim_num(f.native), scale)
        }
        Unit::Unknown => trim_num(f.native).to_string(),
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(metric: &str, value: f64, scale: AmountScale, period: Period, scope: Scope) -> EvidenceFact {
        EvidenceFact {
            metric_id: Some(metric.to_string()),
            base_metric_id: None,
            value: value * scale.factor(),
            native: value,
            unit: Unit::Amount,
            scale,
            period,
            scope,
            source: FactSource::Metric,
            label: metric_name(metric).to_string(),
            locator: "test".into(),
        }
    }

    fn pct(metric: &str, value: f64, period: Period, scope: Scope) -> EvidenceFact {
        EvidenceFact {
            metric_id: Some(metric.to_string()),
            base_metric_id: None,
            value,
            native: value,
            unit: Unit::Percent,
            scale: AmountScale::Unknown,
            period,
            scope,
            source: FactSource::Metric,
            label: metric_name(metric).to_string(),
            locator: "test".into(),
        }
    }

    fn cue(text: &str) -> ClaimCue {
        extract_cues(text, Period::unknown())
            .into_iter()
            .next()
            .expect("one number")
    }

    #[test]
    fn alias_table_has_no_duplicate_text() {
        let mut seen = std::collections::HashSet::new();
        for a in ALIASES {
            assert!(seen.insert(a.text), "duplicate alias: {}", a.text);
            assert!(!a.id.is_empty());
        }
    }

    #[test]
    fn longest_alias_wins() {
        let c = cue("扣非净利润5042.06万元");
        assert_eq!(c.metric_id.as_deref(), Some("net_profit_deducted"));
        assert_eq!(c.scope, Scope::Deducted);
        assert_eq!(c.unit, Unit::Amount);
        assert_eq!(c.scale, AmountScale::Wan);
        assert!((c.value - 50_420_600.0).abs() < 1.0);
    }

    #[test]
    fn parent_scope_is_read_from_the_alias() {
        let c = cue("归母净利润6693.78万元");
        assert_eq!(c.metric_id.as_deref(), Some("net_profit"));
        assert_eq!(c.scope, Scope::Parent);
    }

    #[test]
    fn growth_rate_uses_yoy_not_the_base_metric() {
        let c = cue("归母净利+47.86%");
        assert_eq!(c.metric_id.as_deref(), Some("yoy_first"));
        assert_eq!(c.base_metric_id.as_deref(), Some("net_profit"));
        assert_eq!(c.unit, Unit::Percent);
    }

    #[test]
    fn margin_is_not_mistaken_for_growth() {
        let c = cue("净利率约2.7%");
        assert_eq!(c.metric_id.as_deref(), Some("net_margin_pct"));
        assert_eq!(c.unit, Unit::Percent);
    }

    #[test]
    fn periods_parse() {
        assert_eq!(
            parse_period("2026Q3营业收入"),
            Some(Period {
                year: Some(2026),
                span: PeriodSpan::Q3
            })
        );
        assert_eq!(
            parse_period("2026年第三季度报告"),
            Some(Period {
                year: Some(2026),
                span: PeriodSpan::Q3
            })
        );
        assert_eq!(
            parse_period("2026年前三季度"),
            Some(Period {
                year: Some(2026),
                span: PeriodSpan::M9
            })
        );
        assert_eq!(
            parse_period("2026年上半年"),
            Some(Period {
                year: Some(2026),
                span: PeriodSpan::H1
            })
        );
        assert_eq!(
            parse_period("2026年度"),
            Some(Period {
                year: Some(2026),
                span: PeriodSpan::Fy
            })
        );
        assert_eq!(
            parse_period("Q3单季"),
            Some(Period {
                year: None,
                span: PeriodSpan::Q3
            })
        );
    }

    #[test]
    fn periods_compare_only_when_comparable() {
        let q3 = Period {
            year: Some(2026),
            span: PeriodSpan::Q3,
        };
        let q2 = Period {
            year: Some(2026),
            span: PeriodSpan::Q2,
        };
        let unknown = Period::unknown();
        assert!(q3.compatible(q2) == false);
        assert!(q3.is_comparable(q2));
        assert!(!q3.is_comparable(unknown));
        assert!(q3.compatible(unknown));
    }

    #[test]
    fn units_normalise_amounts_to_yuan() {
        let c = cue("营业收入24.83亿元");
        assert_eq!(c.unit, Unit::Amount);
        assert_eq!(c.scale, AmountScale::Yi);
        assert!((c.value - 2_483_000_000.0).abs() < 1.0);

        let c = cue("营业收入24.83万元");
        assert_eq!(c.scale, AmountScale::Wan);
        assert!((c.value - 248_300.0).abs() < 1.0);

        let c = cue("营业收入2,482,691,849.55元");
        assert_eq!(c.scale, AmountScale::Yuan);
        assert!((c.value - 2_482_691_849.55).abs() < 1.0);
    }

    #[test]
    fn supported_when_value_metric_period_scope_match() {
        let c = cue("2026Q3营业收入24.83亿元");
        let f = amount(
            "revenue",
            24.83,
            AmountScale::Yi,
            Period {
                year: Some(2026),
                span: PeriodSpan::Q3,
            },
            Scope::Consolidated,
        );
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert_eq!(v, BindingVerdict::Supported);
    }

    #[test]
    fn mismatch_when_account_differs() {
        let c = cue("归母净利润2.15亿元");
        let f = amount(
            "net_profit_deducted",
            2.15,
            AmountScale::Yi,
            Period::unknown(),
            Scope::Deducted,
        );
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert!(matches!(
            v,
            BindingVerdict::Mismatch {
                dimension: BindingDimension::Metric,
                ..
            }
        ));
    }

    #[test]
    fn mismatch_when_quarter_differs() {
        let c = cue("2026Q3营业收入24.83亿元");
        let f = amount(
            "revenue",
            24.83,
            AmountScale::Yi,
            Period {
                year: Some(2026),
                span: PeriodSpan::Q2,
            },
            Scope::Unknown,
        );
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert!(matches!(
            v,
            BindingVerdict::Mismatch {
                dimension: BindingDimension::Period,
                ..
            }
        ));
    }

    #[test]
    fn mismatch_when_scope_differs() {
        let c = cue("合并营业收入24.83亿元");
        let f = amount(
            "revenue",
            24.83,
            AmountScale::Yi,
            Period::unknown(),
            Scope::Parent,
        );
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert!(matches!(
            v,
            BindingVerdict::Mismatch {
                dimension: BindingDimension::Scope,
                ..
            }
        ));
    }

    #[test]
    fn mismatch_when_base_metric_differs() {
        let c = cue("营收+47.86%");
        let mut f = pct("yoy_first", 47.86, Period::unknown(), Scope::Unknown);
        f.base_metric_id = Some("net_profit".into());
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert!(matches!(
            v,
            BindingVerdict::Mismatch {
                dimension: BindingDimension::BaseMetric,
                ..
            }
        ));
    }

    #[test]
    fn value_only_when_no_identity_cue() {
        let c = cue("约2.7%");
        let f = pct("net_margin_pct", 2.7, Period::unknown(), Scope::Unknown);
        let (v, _) = resolve(&c, &[(0, &f)]);
        assert_eq!(v, BindingVerdict::ValueOnly);
    }

    #[test]
    fn a_specific_fact_beats_an_unconstrained_one() {
        // The claim says revenue grew 47.86%, but the only specific fact says
        // that 47.86% is net-profit growth. The unconstrained fact (no base
        // metric) must not hide the more informative mismatch.
        let c = cue("营收+47.86%");
        let unconstrained = pct("yoy_first", 47.86, Period::unknown(), Scope::Unknown);
        let mut specific = pct("yoy_first", 47.86, Period::unknown(), Scope::Unknown);
        specific.base_metric_id = Some("net_profit".into());
        let (v, _) = resolve(&c, &[(0, &unconstrained), (1, &specific)]);
        assert!(matches!(
            v,
            BindingVerdict::Mismatch {
                dimension: BindingDimension::BaseMetric,
                ..
            }
        ));
    }

    #[test]
    fn facts_from_metrics_skips_booleans_and_infers_percentages() {
        let mut m = BTreeMap::new();
        m.insert("revenue".into(), Value::from(24.83));
        m.insert("net_margin_pct".into(), Value::from(2.7));
        m.insert("deducted_lte".into(), Value::Bool(true));
        let facts = facts_from_metrics(&m, "亿元", Period::unknown(), FactSource::Calc);
        assert_eq!(facts.len(), 2);
        let revenue = facts.iter().find(|f| f.metric_id.as_deref() == Some("revenue")).unwrap();
        assert_eq!(revenue.unit, Unit::Amount);
        assert_eq!(revenue.scale, AmountScale::Yi);
        assert!((revenue.value - 2_483_000_000.0).abs() < 1.0);
        let margin = facts
            .iter()
            .find(|f| f.metric_id.as_deref() == Some("net_margin_pct"))
            .unwrap();
        assert_eq!(margin.unit, Unit::Percent);
    }

    #[test]
    fn facts_from_text_reads_the_excerpt_shape() {
        let excerpt = "营业收入 2,482,691,849.55 元（同比 +10.90%）；归属于上市公司股东的净利润 66,937,816.48 元（同比 +47.86%）；归属于上市公司股东的扣除非经常性损益的净利润 50,420,578.11 元（同比 +98.64%）。";
        let facts = facts_from_text(excerpt, Period::unknown(), FactSource::Excerpt, "excerpt");
        assert!(facts.iter().any(|f| f.metric_id.as_deref() == Some("revenue")
            && (f.value - 2_482_691_849.55).abs() < 1.0));
        assert!(facts.iter().any(|f| f.metric_id.as_deref() == Some("net_profit")
            && f.scope == Scope::Parent
            && (f.value - 66_937_816.48).abs() < 1.0));
        assert!(facts.iter().any(|f| f.metric_id.as_deref() == Some("net_profit_deducted")
            && f.scope == Scope::Deducted
            && (f.value - 50_420_578.11).abs() < 1.0));
        assert!(facts.iter().any(|f| f.metric_id.as_deref() == Some("yoy_first")
            && f.unit == Unit::Percent
            && (f.value - 47.86).abs() < 1e-9));
    }

    #[test]
    fn dto_facts_keep_unit_period_and_scope() {
        let dto = vec![EvidenceFactDto {
            metric_id: "net_profit".into(),
            metric_name: "归母净利润".into(),
            value: 0.669378,
            unit: "亿元".into(),
            period: "2026M9".into(),
            scope: "parent".into(),
            locator: "PDF 第 2 页".into(),
            ..Default::default()
        }];
        let facts = facts_from_dto(&dto, FactSource::Excerpt);
        let f = &facts[0];
        assert_eq!(f.metric_id.as_deref(), Some("net_profit"));
        assert_eq!(f.scope, Scope::Parent);
        assert_eq!(
            f.period,
            Period {
                year: Some(2026),
                span: PeriodSpan::M9
            }
        );
        assert!((f.value - 66_937_800.0).abs() < 1.0);
        assert_eq!(f.locator, "PDF 第 2 页");
    }
}
