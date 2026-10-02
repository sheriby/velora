//! YAML front matter 的共享解析。
//!
//! 编辑器的属性卡渲染与 HTML 导出（GitHub 风格 key/value 表）共用同一套
//! 行解析，保证「编辑器里看到什么」和「导出写了什么」来自同一份数据。
//! 刻意不做完整 YAML 解析：front matter 展示只承担一眼能读的元数据概览，
//! 编辑与保存始终针对原始字节。

/// 剥掉文档开头的 `---` 围栏对，返回围栏内的 YAML 主体。
/// 围栏不完整（没有关闭行）时原样返回。
pub(crate) fn front_matter_body(raw: &str) -> String {
    let body = raw
        .strip_prefix("---\r\n")
        .or_else(|| raw.strip_prefix("---\n"))
        .unwrap_or(raw);
    let body = body
        .strip_suffix("\r\n---")
        .or_else(|| body.strip_suffix("\n---"))
        .unwrap_or(body);
    body.to_string()
}

/// 把 YAML 主体解析成顶层 key → 值列表：
/// - 顶层 `key: value` 开新行；value 为空时由后续缩进行/列表项填充；
/// - 列表项（`- x`）与缩进续行并入上一个 key 的值列表。
pub(crate) fn parse_front_matter_rows(body: &str) -> Vec<(String, Vec<String>)> {
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !line.starts_with([' ', '\t'])
            && let Some((key, value)) = trimmed.split_once(':')
        {
            let mut values = Vec::new();
            let value = value.trim();
            // YAML 块标量指示符（>、|、>-、|+ …）本身不是内容，跳过，
            // 让后续缩进行充当值——否则属性面板会渲染出一个孤零零的 ">"。
            if !value.is_empty() && !is_block_scalar_indicator(value) {
                values.push(value.to_string());
            }
            rows.push((key.trim().to_string(), values));
            continue;
        }
        let value = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim();
        if !value.is_empty()
            && let Some(row) = rows.last_mut()
        {
            row.1.push(value.to_string());
        }
    }
    rows
}

/// `>`/`|` 加可选的 `+`/`-`/数字修饰（如 `>-`、`|2`）是 YAML 块标量指示符。
fn is_block_scalar_indicator(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('|') | Some('>'))
        && chars.all(|ch| matches!(ch, '+' | '-' | '0'..='9'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_body_strips_fences_both_newline_styles() {
        assert_eq!(front_matter_body("---\nname: x\n---"), "name: x");
        assert_eq!(front_matter_body("---\r\nname: x\r\n---"), "name: x");
        assert_eq!(front_matter_body("name: x"), "name: x");
    }

    #[test]
    fn parse_front_matter_rows_groups_scalars_lists_and_nested_lines() {
        let body = "name: agent\ntemperature: 0.1\ntools:\n  write: true\nskills:\n  - a\n  - b";
        let rows = parse_front_matter_rows(body);

        assert_eq!(rows[0], ("name".into(), vec!["agent".into()]));
        assert_eq!(rows[1], ("temperature".into(), vec!["0.1".into()]));
        assert_eq!(
            rows[2],
            ("tools".into(), vec!["write: true".into()])
        );
        assert_eq!(
            rows[3],
            ("skills".into(), vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn parse_front_matter_rows_skips_stray_lines_before_any_key() {
        assert!(parse_front_matter_rows("- orphan\nkey: v")[0].0 == "key");
    }

    #[test]
    fn parse_front_matter_rows_drops_block_scalar_indicators() {
        let body = "argument-hint: >\n  输入格式: \"生成算子\"\n  参数: npu";
        let rows = parse_front_matter_rows(body);

        assert_eq!(rows[0].0, "argument-hint");
        assert_eq!(
            rows[0].1,
            vec!["输入格式: \"生成算子\"", "参数: npu"],
            "块标量指示符 > 不应出现在值里"
        );
    }
}
