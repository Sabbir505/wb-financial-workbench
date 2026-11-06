"""mock 数据敏感信息扫描器：打包/公开前必跑，零命中才允许发布。

    python check_privacy.py [目录，默认当前目录]

扫描规则（命中任一即判失败）：
1. API 密钥/令牌形态：sk-xxx、Bearer xxx、api_key/token/secret/password 赋值为非占位值
2. 临时隧道地址：*.trycloudflare.com（随时间失效且指向内网）
3. 个人信息形态：手机号（1[3-9]\d{9}）、身份证（\d{17}[\dXx]）、邮箱
4. .env 文件或 .env.local 内容混入

数据合规基线：本包数据全部来自巨潮资讯/深交所互动易等**公开披露渠道**，
公司名/股东名（控股股东为披露义务人）/财务数字均为法定公开信息，不属于敏感数据。
"""

import re
import sys
from pathlib import Path

RULES = [
    ("API 密钥形态 sk-*", re.compile(r"sk-[A-Za-z0-9]{12,}")),
    ("Bearer 令牌", re.compile(r"Bearer\s+[A-Za-z0-9_\-.]{16,}")),
    ("疑似密钥赋值", re.compile(r"(?i)(api[_-]?key|token|secret|password)\s*[:=]\s*[\"']?(?!占位|PLACEHOLDER|xxx|YOUR)[A-Za-z0-9_\-.]{12,}")),
    ("隧道地址", re.compile(r"[a-z0-9-]+\.trycloudflare\.com")),
    # 数字两侧不能有字母或数字（排除 Cargo.lock 这类十六进制哈希串的误报）
    ("手机号", re.compile(r"(?<![\da-zA-Z])1[3-9]\d{9}(?![\da-zA-Z])")),
    ("身份证号", re.compile(r"(?<![\da-zA-Z])\d{17}[\dXx](?![\da-zA-Z])")),
    ("邮箱", re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")),
    ("env 内容混入", re.compile(r"AML_LLM_[A-Z_]+\s*=\s*\S+")),
]

# 白名单：合法出现但会被规则误伤的内容（逐条注明理由）
WHITELIST = [
    (re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
     re.compile(r"^https?://")),  # URL 不是邮箱
]


def scan(root: Path) -> list[tuple[str, str, str]]:
    findings = []
    for f in sorted(root.rglob("*")):
        if not f.is_file() or f.name == "check_privacy.py":
            continue
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for name, pat in RULES:
            for m in pat.finditer(text):
                hit = m.group(0)
                if any(w.search(hit) for _, w in WHITELIST if w.pattern):
                    continue
                line = text[:m.start()].count("\n") + 1
                findings.append((f.name, f"{name} @行{line}", hit[:40]))
    return findings


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent
    findings = scan(root)
    if findings:
        print(f"发现 {len(findings)} 处疑似敏感数据：")
        for fn, rule, hit in findings:
            print(f"  {fn} | {rule} | {hit}")
        return 1
    print(f"扫描通过：{root.name} 下无敏感数据命中（{len(list(root.rglob('*')))} 个条目）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
