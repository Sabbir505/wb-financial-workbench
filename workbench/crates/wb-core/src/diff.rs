//! Line-based LCS diff for the 修改后通过 (approve-as-revised) view (F6).
//! The original draft is never touched; the diff is display-only.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffOp {
    Same,
    Insert,
    Delete,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub op: DiffOp,
    pub text: String,
    /// Line number in the original (Delete/Same), 1-based.
    pub old_no: Option<usize>,
    /// Line number in the revised (Insert/Same), 1-based.
    pub new_no: Option<usize>,
}

pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();

    // LCS table
    let n = a.len();
    let m = b.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(DiffLine {
                op: DiffOp::Same,
                text: a[i].to_string(),
                old_no: Some(i + 1),
                new_no: Some(j + 1),
            });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(DiffLine {
                op: DiffOp::Delete,
                text: a[i].to_string(),
                old_no: Some(i + 1),
                new_no: None,
            });
            i += 1;
        } else {
            out.push(DiffLine {
                op: DiffOp::Insert,
                text: b[j].to_string(),
                old_no: None,
                new_no: Some(j + 1),
            });
            j += 1;
        }
    }
    while i < n {
        out.push(DiffLine {
            op: DiffOp::Delete,
            text: a[i].to_string(),
            old_no: Some(i + 1),
            new_no: None,
        });
        i += 1;
    }
    while j < m {
        out.push(DiffLine {
            op: DiffOp::Insert,
            text: b[j].to_string(),
            old_no: None,
            new_no: Some(j + 1),
        });
        j += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_add_delete_same() {
        let old = "经营变化：前三季营收+10.90%、归母净利+47.86%、扣非+98.64%；Q3单季增速明显回落；在建工程+200.71%，扩产项目进入建设期。\n风险点：经营现金流-7483.90万元（-127.02%），存货及应收账款占用增加；净利率约2.7%，盈利偏薄；扣非低于归母约1652万元。\n待核实问题：非经常性损益具体构成。";
        let new = "经营变化：前三季营收+10.90%、归母净利+47.86%、扣非+98.64%；Q3单季增速明显回落；在建工程+200.71%，扩产项目进入建设期。\n风险点：经营现金流-7483.90万元（-127.02%），存货及应收账款占用增加；净利率约2.7%，盈利偏薄。\n待核实问题：非经常性损益具体构成。";

        let d = diff_lines(old, new);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].op, DiffOp::Same);
        assert_eq!(d[1].op, DiffOp::Delete);
        assert!(d[1].text.contains("1652"));
        assert_eq!(d[2].op, DiffOp::Insert);
        assert!(!d[2].text.contains("1652"));
        assert_eq!(d[3].op, DiffOp::Same);
    }

    #[test]
    fn pure_insert_and_delete() {
        assert_eq!(diff_lines("a\nb", "a\nb\nc").len(), 3);
        assert_eq!(
            diff_lines("a\nb", "a\nb\nc").last().unwrap().op,
            DiffOp::Insert
        );
        assert_eq!(diff_lines("a\nb\nc", "a\nb").len(), 3);
        assert_eq!(
            diff_lines("a\nb\nc", "a\nb").last().unwrap().op,
            DiffOp::Delete
        );
        assert!(diff_lines("", "x").first().unwrap().op == DiffOp::Insert);
        assert!(diff_lines("x", "").first().unwrap().op == DiffOp::Delete);
    }
}
