"""Generate the planted-violation corpus from upstream api_mock fixtures.

Evidence facts are built ONLY from the deterministic metrics file and the
announcement excerpt. The brief's CALCULATION section is never used, so a
value check cannot verify the brief against itself.

Run: python workbench/eval/generate_corpus.py
"""
import json, os, re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MOCK = os.path.join(os.path.dirname(ROOT), "Ref+Fun+MockData", "api_mock")
OUT = os.path.join(ROOT, "eval", "planted-corpus-v1.json")

BASES = [
    "response", "fenghua", "jiemei", "jizhi", "shuangxing",
    "yongguan_failed", "deguan", "luxshare_v2",
]
SCOPE = {
    "revenue": "consolidated",
    "net_profit": "parent",
    "net_profit_deducted": "deducted",
    "ocf": "consolidated",
}


def load(name):
    with open(os.path.join(MOCK, name), encoding="utf-8-sig") as f:
        return json.load(f)


def amount_values(metrics, excerpt):
    vals = [float(v) for k, v in metrics.items()
            if isinstance(v, (int, float)) and not k.endswith("_pct")
            and k not in ("yoy_first", "pct_change_event")]
    for tok in re.findall(r"\d[\d,]*\.?\d*", excerpt):
        n = float(tok.replace(",", ""))
        if n >= 1000:
            vals.append(n / 1e8)
        elif "亿" in excerpt[max(0, excerpt.find(tok) - 4):excerpt.find(tok) + len(tok) + 2]:
            vals.append(n)
    return vals


def safe(value, amounts):
    return all(abs(value - a) > 0.012 * max(abs(a), 1e-9) for a in amounts)


def fmt(v):
    return f"{v:.4f}".rstrip("0").rstrip(".")


def build_case(base, task, report, indicators, variant, inference, raw, value_exp, binding_exp, note):
    facts = []
    for k, v in indicators["metrics"].items():
        if isinstance(v, bool):
            continue
        is_pct = k.endswith("_pct") or k in ("yoy_first", "pct_change_event")
        facts.append({
            "metric_id": k,
            "metric_name": k,
            "base_metric_id": "",
            "value": v,
            "unit": "%" if is_pct else "亿元",
            "period": "2026M9",
            "scope": "" if is_pct else SCOPE.get(k, "consolidated"),
            "locator": "公告主要会计数据",
            "source": "cninfo",
        })
    return {
        "id": f"{base}-{variant}",
        "base_task": task["task_id"],
        "condition": variant,
        "note": note,
        "inference": inference,
        "evidence": {
            "event_id": report.get("event_id", ""),
            "source_url": report.get("source_url", ""),
            "excerpt": report.get("content_excerpt", ""),
            "locator": "公告摘录",
            "fetched_at": report.get("event_date", ""),
            "facts": facts,
        },
        "labels": [{"raw": raw, "value": value_exp, "binding": binding_exp}],
    }


cases = []
for base in BASES:
    task = load(f"analysis_task.{base}.json")
    report = load(f"financial_reports.{base}.json")
    indicators = load(f"financial_indicators.{base}.json")
    metrics = {k: v for k, v in indicators["metrics"].items()
               if isinstance(v, (int, float)) and not isinstance(v, bool)}
    if "revenue" not in metrics or "net_profit" not in metrics:
        continue
    rev = float(metrics["revenue"])
    net = float(metrics["net_profit"])
    excerpt = report.get("content_excerpt", "")
    amounts = amount_values(metrics, excerpt)

    clean_v = rev * 1.005
    near_v = rev * 1.02
    invented = 1652.0
    while not safe(invented / 1e4, amounts):
        invented += 137.0

    specs = [
        ("clean_amount", f"营业收入{fmt(rev)}亿元。", fmt(rev), "pass", "ok",
         "real amount, must not be blocked"),
        ("invented_difference", f"扣非低于归母约{fmt(invented)}万元。", fmt(invented),
         "flag", "unsupported", "model-computed number absent from evidence"),
        ("near_miss_outside", f"营业收入{fmt(near_v)}亿元。", fmt(near_v), "flag",
         "unsupported", "2% above the evidence amount, outside the 1% band"),
        ("within_tolerance", f"营业收入{fmt(clean_v)}亿元。", fmt(clean_v), "pass",
         "ok", "0.5% above the evidence amount, inside the tolerance"),
        ("wrong_account", f"营业收入{fmt(net)}亿元。", fmt(net), "pass", "mismatch",
         "net profit value attributed to revenue"),
        ("wrong_period", f"2026Q2营业收入{fmt(rev)}亿元。", fmt(rev), "pass",
         "mismatch", "Q2 claim against an M9 fact"),
        ("wrong_scope", f"合并口径净利润{fmt(net)}亿元。", fmt(net), "pass",
         "mismatch", "consolidated claim against a parent-scope fact"),
    ]
    for variant, text, raw, ve, be, note in specs:
        cases.append(build_case(base, task, report, indicators, variant, text, raw, ve, be, note))

with open(OUT, "w", encoding="utf-8") as f:
    json.dump({
        "version": "1.0",
        "created": "2026-11-22",
        "source": "api_mock financial_reports.*.json (excerpt) and financial_indicators.*.json (deterministic metrics)",
        "rule": "No evidence fact is derived from the brief's CALCULATION section.",
        "cases": cases,
    }, f, ensure_ascii=False, indent=2)
print(f"wrote {len(cases)} cases to {OUT}")
