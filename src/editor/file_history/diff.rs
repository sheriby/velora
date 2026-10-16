#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiffKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug)]
pub(super) struct DiffLine {
    pub(super) kind: DiffKind,
    pub(super) old_line: Option<usize>,
    pub(super) new_line: Option<usize>,
    pub(super) text: String,
}

pub(super) fn diff_lines(before: &str, after: &str) -> Vec<DiffLine> {
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let prefix = old
        .iter()
        .zip(&new)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = old
        .iter()
        .skip(prefix)
        .rev()
        .zip(new.iter().skip(prefix).rev())
        .take_while(|(left, right)| left == right)
        .count();
    // 首尾已经逐行验证，两个中段范围不会交叉或超出各自长度。
    let old_middle = &old[prefix..old.len() - suffix];
    let new_middle = &new[prefix..new.len() - suffix];
    let mut changes = Vec::new();
    changes.extend(
        old.iter()
            .take(prefix)
            .map(|text| (DiffKind::Context, *text)),
    );
    let height = old_middle.len() + 1;
    let width = new_middle.len() + 1;
    // 完全改写的大文档按整段删除/增加展示，避免差异计算占用平方级内存。
    if height.saturating_mul(width) <= 1_000_000 {
        let mut lengths = vec![0usize; height * width];
        for row in (0..old_middle.len()).rev() {
            for column in (0..new_middle.len()).rev() {
                // row/column 均小于中段长度，后一行/列最多落在补出的边界单元。
                lengths[row * width + column] = if old_middle[row] == new_middle[column] {
                    lengths[(row + 1) * width + column + 1] + 1
                } else {
                    lengths[(row + 1) * width + column].max(lengths[row * width + column + 1])
                };
            }
        }
        let (mut row, mut column) = (0, 0);
        while row < old_middle.len() && column < new_middle.len() {
            if old_middle[row] == new_middle[column] {
                changes.push((DiffKind::Context, old_middle[row]));
                row += 1;
                column += 1;
            } else if lengths[(row + 1) * width + column] >= lengths[row * width + column + 1] {
                changes.push((DiffKind::Removed, old_middle[row]));
                row += 1;
            } else {
                changes.push((DiffKind::Added, new_middle[column]));
                column += 1;
            }
        }
        changes.extend(
            old_middle
                .iter()
                .skip(row)
                .map(|text| (DiffKind::Removed, *text)),
        );
        changes.extend(
            new_middle
                .iter()
                .skip(column)
                .map(|text| (DiffKind::Added, *text)),
        );
    } else {
        changes.extend(old_middle.iter().map(|text| (DiffKind::Removed, *text)));
        changes.extend(new_middle.iter().map(|text| (DiffKind::Added, *text)));
    }
    changes.extend(
        old.iter()
            .skip(old.len() - suffix)
            .map(|text| (DiffKind::Context, *text)),
    );
    let (mut old_line, mut new_line) = (0, 0);
    changes
        .into_iter()
        .map(|(kind, text)| {
            let old_number = if kind != DiffKind::Added {
                old_line += 1;
                Some(old_line)
            } else {
                None
            };
            let new_number = if kind != DiffKind::Removed {
                new_line += 1;
                Some(new_line)
            } else {
                None
            };
            DiffLine {
                kind,
                old_line: old_number,
                new_line: new_number,
                text: text.into(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{DiffKind, diff_lines};
    #[test]
    fn comparison_marks_insertions_deletions_and_preserves_unicode() {
        let lines = diff_lines("原文\n保留\n旧行\n", "新增\n原文\n保留\n新行\n");
        assert!(
            lines
                .iter()
                .any(|line| line.kind == DiffKind::Added && line.text == "新增\n")
        );
        assert!(
            lines
                .iter()
                .any(|line| line.kind == DiffKind::Removed && line.text == "旧行\n")
        );
        assert!(
            lines
                .iter()
                .any(|line| line.kind == DiffKind::Context && line.text == "保留\n")
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind != DiffKind::Added)
                .map(|line| line.text.as_str())
                .collect::<String>(),
            "原文\n保留\n旧行\n"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind != DiffKind::Removed)
                .map(|line| line.text.as_str())
                .collect::<String>(),
            "新增\n原文\n保留\n新行\n"
        );
    }
    #[test]
    fn comparison_reconstructs_empty_identical_and_newline_sensitive_inputs() {
        for (before, after) in [
            ("", ""),
            ("", "新\n"),
            ("旧", ""),
            ("相同\n", "相同\n"),
            ("行\n", "行"),
            ("a\nb\nc\n", "c\na\nb\n"),
        ] {
            let lines = diff_lines(before, after);
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.kind != DiffKind::Added)
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                before
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.kind != DiffKind::Removed)
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                after
            );
            let old_numbers: Vec<_> = lines.iter().filter_map(|line| line.old_line).collect();
            let new_numbers: Vec<_> = lines.iter().filter_map(|line| line.new_line).collect();
            assert_eq!(
                old_numbers,
                (1..=before.split_inclusive('\n').count()).collect::<Vec<_>>()
            );
            assert_eq!(
                new_numbers,
                (1..=after.split_inclusive('\n').count()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn comparison_of_large_rewrites_is_complete_with_bounded_work() {
        let before = "旧内容\n".repeat(10000);
        let after = "新内容\n".repeat(10000);
        let lines = diff_lines(&before, &after);
        assert_eq!(lines.len(), 20000);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind == DiffKind::Removed)
                .count(),
            10000
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind == DiffKind::Added)
                .count(),
            10000
        );
    }
}
