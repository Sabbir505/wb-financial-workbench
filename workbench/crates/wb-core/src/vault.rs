//! I2 vault file contract: `{vault_root}/research/{yyyy-mm}/{task_id}.md`
//! — 13 frontmatter fields + four fixed body sections
//! (事实 FACT / 计算 CALCULATION / 推断 INFERENCE / 校验 CHECK).

use crate::model::{BriefSections, Frontmatter};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    Fact,
    Calculation,
    Inference,
    Check,
}

impl SectionKind {
    pub fn label(&self) -> &'static str {
        match self {
            SectionKind::Fact => "事实 FACT",
            SectionKind::Calculation => "计算 CALCULATION",
            SectionKind::Inference => "推断 INFERENCE",
            SectionKind::Check => "校验 CHECK",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    pub kind: SectionKind,
    pub heading: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Brief {
    pub path: PathBuf,
    pub frontmatter: Frontmatter,
    pub title: String,
    pub sections: Vec<Section>,
}

impl Brief {
    pub fn section(&self, kind: SectionKind) -> Option<&Section> {
        self.sections.iter().find(|s| s.kind == kind)
    }

    /// Guardrail input from the CALCULATION section (deterministic tool output).
    pub fn calc_values(&self) -> BTreeMapOwned {
        let sec = self.section(SectionKind::Calculation);
        let mut out = BTreeMapOwned::default();
        if let Some(sec) = sec {
            for line in sec.body.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().trim_start_matches("- ").trim().to_string();
                    let raw = v.trim();
                    if let Ok(n) = raw.replace(',', "").parse::<f64>() {
                        out.insert(key, n);
                    }
                }
            }
        }
        out
    }

    pub fn full_text(&self) -> String {
        let mut s = self.title.clone();
        s.push('\n');
        for sec in &self.sections {
            s.push_str(&sec.heading);
            s.push('\n');
            s.push_str(&sec.body);
            s.push('\n');
        }
        s
    }
}

pub type BTreeMapOwned = std::collections::BTreeMap<String, f64>;

/// Local-first vault store over the research directory.
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Vault {
        Vault { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Canonical brief path for a task (yyyy-mm taken from created_at when available).
    pub fn brief_path(&self, task_id: &str, created_at: &str) -> PathBuf {
        let ym = created_at.get(..7).unwrap_or("1970-01");
        self.root
            .join("research")
            .join(ym)
            .join(format!("{task_id}.md"))
    }

    pub fn write_brief(
        &self,
        brief_md: &str,
        task_id: &str,
        created_at: &str,
    ) -> std::io::Result<PathBuf> {
        let path = self.brief_path(task_id, created_at);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, brief_md)?;
        Ok(path)
    }

    pub fn write_brief_at(&self, rel: &str, content: &str) -> std::io::Result<PathBuf> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Locate a brief by task_id anywhere under the vault root.
    pub fn find_brief_path(&self, task_id: &str) -> Option<PathBuf> {
        let target = format!("{task_id}.md");
        let mut found = None;
        walk_md(&self.root, &mut |p| {
            if found.is_none() && p.file_name().map(|f| f == target.as_str()).unwrap_or(false) {
                found = Some(p.to_path_buf());
            }
        });
        found
    }

    pub fn read_brief(&self, task_id: &str) -> Option<Brief> {
        let path = self.find_brief_path(task_id)?;
        parse_brief(&path).ok()
    }

    /// All briefs under the vault, newest first by frontmatter created_at.
    pub fn list_briefs(&self) -> Vec<Brief> {
        let mut out = Vec::new();
        walk_md(&self.root, &mut |p| {
            if let Ok(b) = parse_brief(p) {
                out.push(b);
            }
        });
        out.sort_by_key(|b| std::cmp::Reverse(b.frontmatter.created_at.clone()));
        out
    }

    /// Full-text search (F1): case-insensitive, scored by occurrences, title hits weigh 10x.
    pub fn search(&self, query: &str) -> Vec<(Brief, usize)> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.list_briefs().into_iter().map(|b| (b, 0)).collect();
        }
        let mut hits: Vec<(Brief, usize)> = Vec::new();
        for brief in self.list_briefs() {
            let title = brief.title.to_lowercase();
            let body = brief.full_text().to_lowercase();
            let score = count_occurrences(&body, &q) + count_occurrences(&title, &q) * 10;
            if score > 0 {
                hits.push((brief, score));
            }
        }
        hits.sort_by_key(|h| std::cmp::Reverse(h.1));
        hits
    }
}

fn count_occurrences(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut rest = hay;
    while let Some(i) = rest.find(needle) {
        n += 1;
        rest = &rest[i + needle.len()..];
    }
    n
}

fn walk_md(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_md(&path, f);
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            f(&path);
        }
    }
}

/// Parse a brief file: `---` frontmatter block, `# title`, `## section` bodies.
pub fn parse_brief(path: &Path) -> Result<Brief, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text.as_str());
    parse_brief_str(text, path)
}

pub fn parse_brief_str(text: &str, path: &Path) -> Result<Brief, String> {
    let mut rest = text;
    let mut fm = Frontmatter::default();

    if let Some(after_open) = text.strip_prefix("---") {
        if let Some(end) = after_open.find("\n---") {
            let yaml = &after_open[..end];
            fm = parse_frontmatter(yaml);
            rest = &after_open[end + 4..];
        }
    }

    let mut title = String::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut current_heading = String::new();
    let mut current_kind: Option<SectionKind> = None;
    let mut current_body = String::new();

    for line in rest.lines() {
        if let Some(h1) = line.strip_prefix("# ") {
            title = h1.trim().to_string();
            continue;
        }
        if let Some(h2) = line.strip_prefix("## ") {
            if let Some(kind) = current_kind.take() {
                sections.push(Section {
                    kind,
                    heading: current_heading.clone(),
                    body: current_body.trim().to_string(),
                });
            }
            let heading = h2.trim().to_string();
            current_kind = classify_heading(&heading);
            current_heading = heading;
            current_body.clear();
            continue;
        }
        if current_kind.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(kind) = current_kind {
        sections.push(Section {
            kind,
            heading: current_heading,
            body: current_body.trim().to_string(),
        });
    }

    Ok(Brief {
        path: path.to_path_buf(),
        frontmatter: fm,
        title,
        sections,
    })
}

fn classify_heading(heading: &str) -> Option<SectionKind> {
    if heading.contains("FACT") || heading.contains("事实") {
        Some(SectionKind::Fact)
    } else if heading.contains("CALCULATION") || heading.contains("计算") {
        Some(SectionKind::Calculation)
    } else if heading.contains("INFERENCE") || heading.contains("推断") {
        Some(SectionKind::Inference)
    } else if heading.contains("CHECK") || heading.contains("校验") {
        Some(SectionKind::Check)
    } else {
        None
    }
}

/// Minimal frontmatter parser: `key: value` lines, optional double quotes.
pub fn parse_frontmatter(yaml: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    for line in yaml.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        let mut val = v.trim().to_string();
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len() - 1].to_string();
        }
        match key {
            "task_id" => fm.task_id = val,
            "trace_id" => fm.trace_id = val,
            "event_ref" => fm.event_ref = val,
            "object_id" => fm.object_id = val,
            "object_version" => fm.object_version = val,
            "period" => fm.period = val,
            "workflow_version" => fm.workflow_version = val,
            "status" => fm.status = val,
            "review_comment" => fm.review_comment = val,
            "reviewer" => fm.reviewer = val,
            "created_at" => fm.created_at = val,
            "data_cutoff" => fm.data_cutoff = val,
            "source_version" => fm.source_version = val,
            _ => {}
        }
    }
    fm
}

/// Serialize a brief back to the I2 file format (used when applying 修改后通过).
pub fn render_brief_md(fm: &Frontmatter, title: &str, sections: &BriefSections) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("task_id: {}\n", fm.task_id));
    out.push_str(&format!("trace_id: {}\n", fm.trace_id));
    out.push_str(&format!("event_ref: {}\n", fm.event_ref));
    out.push_str(&format!("object_id: {}\n", fm.object_id));
    out.push_str(&format!("object_version: \"{}\"\n", fm.object_version));
    out.push_str(&format!("period: {}\n", fm.period));
    out.push_str(&format!("workflow_version: {}\n", fm.workflow_version));
    out.push_str(&format!("status: {}\n", fm.status));
    out.push_str(&format!("review_comment: \"{}\"\n", fm.review_comment));
    out.push_str(&format!("reviewer: \"{}\"\n", fm.reviewer));
    out.push_str(&format!("created_at: {}\n", fm.created_at));
    out.push_str(&format!("data_cutoff: {}\n", fm.data_cutoff));
    out.push_str(&format!("source_version: {}\n", fm.source_version));
    out.push_str("---\n\n");
    out.push_str(&format!("# {title}\n\n"));
    out.push_str("## 事实（FACT，附证据链接）\n");
    for line in &sections.fact.0 {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("\n## 计算（CALCULATION，确定性工具产出）\n");
    for (k, v) in &sections.calculation.0 {
        out.push_str(&format!("- {k} = {v}\n"));
    }
    out.push_str("\n## 推断（INFERENCE，模型生成）\n");
    out.push_str(sections.inference.trim());
    out.push_str("\n\n## 校验结果与待核实问题\n");
    out.push_str(sections.check.trim());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CalcValues;

    const SAMPLE: &str = "---\ntask_id: RT-PR-EV-20261029-1225636931-S300806\ntrace_id: PR-EV-20261029-1225636931-S300806\nevent_ref: EV-20261029-1225636931\nobject_id: S300806\nobject_version: \"20261029-1225636931\"\nperiod: 2026Q4\nworkflow_version: wf-earnings-v1.0\nstatus: 待审核\nreview_comment: \"\"\nreviewer: \"\"\ncreated_at: 2026-11-02T07:17:03\ndata_cutoff: 2026-11-02T07:12:33\nsource_version: icg-store@2026-10-31T22:25:45\n---\n\n# 斯迪克 2026Q4 财报研究简报\n\n## 事实（FACT，附证据链接）\n- 斯迪克：2026年三季度报告（2026-10-29）\n- 原文：http://static.cninfo.com.cn/finalpage/2026-10-29/1225636931.PDF\n\n## 计算（CALCULATION，确定性工具产出）\n- revenue = 24.83\n- net_profit = 0.669378\n\n## 推断（INFERENCE，模型生成）\n经营变化：前三季营收+10.90%。\n\n## 校验结果与待核实问题\n数字校验未通过：['0.1652']\n";

    #[test]
    fn parses_13_frontmatter_fields_and_4_sections() {
        let brief = parse_brief_str(SAMPLE, Path::new("x.md")).unwrap();
        let fm = &brief.frontmatter;
        assert_eq!(fm.task_id, "RT-PR-EV-20261029-1225636931-S300806");
        assert_eq!(fm.object_id, "S300806");
        assert_eq!(fm.object_version, "20261029-1225636931");
        assert_eq!(fm.period, "2026Q4");
        assert_eq!(fm.status, "待审核");
        assert_eq!(fm.created_at, "2026-11-02T07:17:03");
        assert_eq!(fm.data_cutoff, "2026-11-02T07:12:33");
        assert_eq!(fm.source_version, "icg-store@2026-10-31T22:25:45");
        assert_eq!(fm.trace_id, "PR-EV-20261029-1225636931-S300806");
        assert_eq!(fm.event_ref, "EV-20261029-1225636931");
        assert_eq!(fm.workflow_version, "wf-earnings-v1.0");
        assert_eq!(fm.review_comment, "");
        assert_eq!(fm.reviewer, "");
        assert_eq!(brief.title, "斯迪克 2026Q4 财报研究简报");
        assert_eq!(brief.sections.len(), 4);
        assert_eq!(
            brief
                .section(SectionKind::Fact)
                .unwrap()
                .body
                .lines()
                .count(),
            2
        );
        assert!(brief
            .section(SectionKind::Inference)
            .unwrap()
            .body
            .contains("10.90%"));
        assert!(brief
            .section(SectionKind::Check)
            .unwrap()
            .body
            .contains("0.1652"));
        let calc = brief.calc_values();
        assert_eq!(calc["revenue"], 24.83);
        assert_eq!(calc["net_profit"], 0.669378);
    }

    #[test]
    fn roundtrip_render() {
        let brief = parse_brief_str(SAMPLE, Path::new("x.md")).unwrap();
        let sections = BriefSections {
            fact: crate::model::FactList(vec!["a".into()]),
            calculation: CalcValues::default(),
            inference: "text".into(),
            check: "ok".into(),
        };
        let md = render_brief_md(&brief.frontmatter, &brief.title, &sections);
        let reparsed = parse_brief_str(&md, Path::new("y.md")).unwrap();
        assert_eq!(reparsed.frontmatter, brief.frontmatter);
        assert_eq!(reparsed.sections.len(), 4);
    }

    #[test]
    fn search_scores_title_higher() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = Vault::new(tmp.path());
        vault
            .write_brief_at("research/2026-11/RT-1.md", SAMPLE)
            .unwrap();
        vault
            .write_brief_at(
                "research/2026-11/RT-2.md",
                &SAMPLE
                    .replace("斯迪克", "其他公司")
                    .replace("24.83", "1.23"),
            )
            .unwrap();
        let hits = vault.search("斯迪克");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].0.frontmatter.task_id,
            "RT-PR-EV-20261029-1225636931-S300806"
        );
        let all = vault.search("");
        assert_eq!(all.len(), 2);
        let by_num = vault.search("24.83");
        assert_eq!(by_num.len(), 1);
    }
}
