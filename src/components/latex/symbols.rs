//! LaTeX 命令与符号表：`\` 补全与公式编辑器面板共用的一份静态数据。
//!
//! 每条三件套：`insert` 是确认/点击时写进文档的原文，`caret` 是插入后光标
//! 落在插入文本里的字节偏移（模板类命令落进第一对花括号），`preview` 是
//! 补全列表与面板里交给 ratex 渲染的示例片段。表按「常用在前」排序，补全
//! 截前 8 条就是最常用的那几个。

/// 面板里的分组；补全列表不分组，只按表的顺序出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LatexCategory {
    /// 希腊字母。
    Greek,
    /// 运算符与关系符。
    Operators,
    /// 箭头。
    Arrows,
    /// 结构（分式、根式、求和积分模板、矩阵与多行环境）。
    Structures,
    /// 函数名与排版文字。
    Functions,
    /// 修饰与杂项符号。
    Symbols,
}

/// 一条可补全/可点击插入的 LaTeX 命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LatexSymbol {
    /// 命令名（不含反斜杠），补全查询按它做前缀匹配。
    pub(crate) name: &'static str,
    /// 确认时写进文档的文本（含反斜杠与模板花括号）。
    pub(crate) insert: &'static str,
    /// 插入后光标相对插入文本的字节偏移。
    pub(crate) caret: usize,
    /// 预览渲染用的公式片段（不含定界符）。
    pub(crate) preview: &'static str,
    pub(crate) category: LatexCategory,
}

const fn command(
    name: &'static str,
    insert: &'static str,
    caret: usize,
    preview: &'static str,
    category: LatexCategory,
) -> LatexSymbol {
    LatexSymbol {
        name,
        insert,
        caret,
        preview,
        category,
    }
}

/// 纯符号：插入 `\name `（尾随空格收尾），预览就是命令本身。
macro_rules! symbol {
    ($name:literal, $category:expr) => {
        LatexSymbol {
            name: $name,
            insert: concat!("\\", $name, " "),
            caret: 1 + $name.len() + 1,
            preview: concat!("\\", $name),
            category: $category,
        }
    };
}

pub(crate) const LATEX_SYMBOLS: &[LatexSymbol] = &[
    // ===== 结构：打公式最高频的模板放最前 =====
    command("frac", "\\frac{}{}", 6, "\\frac{a}{b}", LatexCategory::Structures),
    command("sqrt", "\\sqrt{}", 6, "\\sqrt{x}", LatexCategory::Structures),
    command("sum", "\\sum_{}^{}", 6, "\\sum_{i=1}^{n} i", LatexCategory::Structures),
    command("int", "\\int_{}^{}", 6, "\\int_0^1 x dx", LatexCategory::Structures),
    command("lim", "\\lim_{}", 6, "\\lim_{x \\to 0} x", LatexCategory::Structures),
    command("prod", "\\prod_{}^{}", 6, "\\prod_{i=1}^{n} i", LatexCategory::Structures),
    command("begin{cases}", "\\begin{cases} \\\\\n\\end{cases}", 14, "\\begin{cases} a \\\\ b \\end{cases}", LatexCategory::Structures),
    command("begin{pmatrix}", "\\begin{pmatrix} & \\\\ & \\end{pmatrix}", 15, "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", LatexCategory::Structures),
    command("begin{bmatrix}", "\\begin{bmatrix} & \\\\ & \\end{bmatrix}", 15, "\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}", LatexCategory::Structures),
    command("begin{aligned}", "\\begin{aligned} \\\\ \\end{aligned}", 16, "\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}", LatexCategory::Structures),
    // ===== 希腊字母 =====
    symbol!("alpha", LatexCategory::Greek),
    symbol!("beta", LatexCategory::Greek),
    symbol!("gamma", LatexCategory::Greek),
    symbol!("delta", LatexCategory::Greek),
    symbol!("epsilon", LatexCategory::Greek),
    symbol!("varepsilon", LatexCategory::Greek),
    symbol!("zeta", LatexCategory::Greek),
    symbol!("eta", LatexCategory::Greek),
    symbol!("theta", LatexCategory::Greek),
    symbol!("vartheta", LatexCategory::Greek),
    symbol!("iota", LatexCategory::Greek),
    symbol!("kappa", LatexCategory::Greek),
    symbol!("lambda", LatexCategory::Greek),
    symbol!("mu", LatexCategory::Greek),
    symbol!("nu", LatexCategory::Greek),
    symbol!("xi", LatexCategory::Greek),
    symbol!("pi", LatexCategory::Greek),
    symbol!("rho", LatexCategory::Greek),
    symbol!("sigma", LatexCategory::Greek),
    symbol!("tau", LatexCategory::Greek),
    symbol!("upsilon", LatexCategory::Greek),
    symbol!("phi", LatexCategory::Greek),
    symbol!("varphi", LatexCategory::Greek),
    symbol!("chi", LatexCategory::Greek),
    symbol!("psi", LatexCategory::Greek),
    symbol!("omega", LatexCategory::Greek),
    symbol!("Gamma", LatexCategory::Greek),
    symbol!("Delta", LatexCategory::Greek),
    symbol!("Theta", LatexCategory::Greek),
    symbol!("Lambda", LatexCategory::Greek),
    symbol!("Xi", LatexCategory::Greek),
    symbol!("Pi", LatexCategory::Greek),
    symbol!("Sigma", LatexCategory::Greek),
    symbol!("Phi", LatexCategory::Greek),
    symbol!("Psi", LatexCategory::Greek),
    symbol!("Omega", LatexCategory::Greek),
    // ===== 运算与关系 =====
    symbol!("times", LatexCategory::Operators),
    symbol!("div", LatexCategory::Operators),
    symbol!("pm", LatexCategory::Operators),
    symbol!("mp", LatexCategory::Operators),
    symbol!("cdot", LatexCategory::Operators),
    symbol!("circ", LatexCategory::Operators),
    symbol!("star", LatexCategory::Operators),
    symbol!("cup", LatexCategory::Operators),
    symbol!("cap", LatexCategory::Operators),
    symbol!("oplus", LatexCategory::Operators),
    symbol!("otimes", LatexCategory::Operators),
    symbol!("leq", LatexCategory::Operators),
    symbol!("geq", LatexCategory::Operators),
    symbol!("neq", LatexCategory::Operators),
    symbol!("approx", LatexCategory::Operators),
    symbol!("equiv", LatexCategory::Operators),
    symbol!("sim", LatexCategory::Operators),
    symbol!("propto", LatexCategory::Operators),
    symbol!("subseteq", LatexCategory::Operators),
    symbol!("supseteq", LatexCategory::Operators),
    symbol!("in", LatexCategory::Operators),
    symbol!("notin", LatexCategory::Operators),
    symbol!("emptyset", LatexCategory::Operators),
    symbol!("partial", LatexCategory::Operators),
    symbol!("nabla", LatexCategory::Operators),
    symbol!("iint", LatexCategory::Operators),
    symbol!("iiint", LatexCategory::Operators),
    symbol!("oint", LatexCategory::Operators),
    symbol!("ldots", LatexCategory::Operators),
    symbol!("cdots", LatexCategory::Operators),
    symbol!("vdots", LatexCategory::Operators),
    symbol!("ddots", LatexCategory::Operators),
    // ===== 箭头 =====
    symbol!("rightarrow", LatexCategory::Arrows),
    symbol!("leftarrow", LatexCategory::Arrows),
    symbol!("leftrightarrow", LatexCategory::Arrows),
    symbol!("Rightarrow", LatexCategory::Arrows),
    symbol!("Leftarrow", LatexCategory::Arrows),
    symbol!("Leftrightarrow", LatexCategory::Arrows),
    symbol!("mapsto", LatexCategory::Arrows),
    symbol!("uparrow", LatexCategory::Arrows),
    symbol!("downarrow", LatexCategory::Arrows),
    symbol!("hookrightarrow", LatexCategory::Arrows),
    // ===== 函数与文字 =====
    symbol!("sin", LatexCategory::Functions),
    symbol!("cos", LatexCategory::Functions),
    symbol!("tan", LatexCategory::Functions),
    symbol!("cot", LatexCategory::Functions),
    symbol!("sec", LatexCategory::Functions),
    symbol!("csc", LatexCategory::Functions),
    symbol!("arcsin", LatexCategory::Functions),
    symbol!("arccos", LatexCategory::Functions),
    symbol!("arctan", LatexCategory::Functions),
    symbol!("log", LatexCategory::Functions),
    symbol!("ln", LatexCategory::Functions),
    symbol!("exp", LatexCategory::Functions),
    symbol!("min", LatexCategory::Functions),
    symbol!("max", LatexCategory::Functions),
    symbol!("inf", LatexCategory::Functions),
    symbol!("det", LatexCategory::Functions),
    symbol!("gcd", LatexCategory::Functions),
    symbol!("deg", LatexCategory::Functions),
    command("mathbb", "\\mathbb{}", 8, "\\mathbb{R}", LatexCategory::Functions),
    command("mathcal", "\\mathcal{}", 9, "\\mathcal{F}", LatexCategory::Functions),
    command("mathrm", "\\mathrm{}", 8, "\\mathrm{d}x", LatexCategory::Functions),
    command("boldsymbol", "\\boldsymbol{}", 12, "\\boldsymbol{v}", LatexCategory::Functions),
    command("text", "\\text{}", 6, "\\text{where}", LatexCategory::Functions),
    // ===== 修饰与杂项 =====
    command("bar", "\\bar{}", 5, "\\bar{x}", LatexCategory::Symbols),
    command("hat", "\\hat{}", 5, "\\hat{x}", LatexCategory::Symbols),
    command("vec", "\\vec{}", 5, "\\vec{x}", LatexCategory::Symbols),
    command("tilde", "\\tilde{}", 7, "\\tilde{x}", LatexCategory::Symbols),
    command("dot", "\\dot{}", 5, "\\dot{x}", LatexCategory::Symbols),
    command("ddot", "\\ddot{}", 6, "\\ddot{x}", LatexCategory::Symbols),
    command("overline", "\\overline{}", 10, "\\overline{AB}", LatexCategory::Symbols),
    command("underline", "\\underline{}", 11, "\\underline{AB}", LatexCategory::Symbols),
    command("binom", "\\binom{}{}", 7, "\\binom{n}{k}", LatexCategory::Symbols),
    command("overbrace", "\\overbrace{}^{}", 11, "\\overbrace{a+b}^{c}", LatexCategory::Symbols),
    command("underbrace", "\\underbrace{}_{}", 12, "\\underbrace{a+b}_{c}", LatexCategory::Symbols),
    command("dfrac", "\\dfrac{}{}", 7, "\\dfrac{a}{b}", LatexCategory::Structures),
    symbol!("forall", LatexCategory::Symbols),
    symbol!("exists", LatexCategory::Symbols),
    symbol!("angle", LatexCategory::Symbols),
    symbol!("triangle", LatexCategory::Symbols),
    symbol!("perp", LatexCategory::Symbols),
    symbol!("parallel", LatexCategory::Symbols),
    symbol!("because", LatexCategory::Symbols),
    symbol!("therefore", LatexCategory::Symbols),
    symbol!("hbar", LatexCategory::Symbols),
    symbol!("ell", LatexCategory::Symbols),
    symbol!("Re", LatexCategory::Symbols),
    symbol!("Im", LatexCategory::Symbols),
    symbol!("quad", LatexCategory::Symbols),
    symbol!("qquad", LatexCategory::Symbols),
    command("left(", "\\left( ", 7, "\\left( a \\right)", LatexCategory::Symbols),
    command("right)", "\\right) ", 8, "\\left( a \\right)", LatexCategory::Symbols),
];

/// 前缀匹配补全：命令名以 `query`（不含反斜杠）开头，保持表序（常用在前）。
pub(crate) fn latex_completions_for(query: &str, limit: usize) -> Vec<&'static LatexSymbol> {
    if query.is_empty() {
        // 空查询给出打公式最常用的前几条；`\frac`、`\sqrt` 在表头。
        return LATEX_SYMBOLS.iter().take(limit).collect();
    }
    let query_lower = query.to_lowercase();
    LATEX_SYMBOLS
        .iter()
        .filter(|entry| entry.name.to_lowercase().starts_with(&query_lower))
        .take(limit)
        .collect()
}

/// 光标前是不是一条 `\命令`（命令名只含字母），返回 `(反斜杠位置, 命令名)`。
/// 是否处于公式上下文由调用方判定；这里只管词法。`\\` 的第二个反斜杠是
/// 换行转义，不再起命令作用。
pub(crate) fn latex_command_before_cursor(prefix: &str) -> Option<(usize, &str)> {
    let bytes = prefix.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_alphabetic() {
        end -= 1;
    }
    if end == 0 || bytes[end - 1] != b'\\' {
        return None;
    }
    let backslash = end - 1;
    let mut escaped = false;
    let mut i = backslash;
    while i > 0 && bytes[i - 1] == b'\\' {
        escaped = !escaped;
        i -= 1;
    }
    (!escaped).then_some((backslash, &prefix[end..]))
}

/// 行前缀里未配对的 `$` 个数为奇 → 光标在行内公式里（`\$` 转义的不算）。
pub(crate) fn inside_inline_math(prefix: &str) -> bool {
    let mut count = 0usize;
    let bytes = prefix.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
            }
            b'$' => {
                count += 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    count % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_query_returns_common_commands_first() {
        let empty = latex_completions_for("", 8);
        assert_eq!(empty.len(), 8);
        assert_eq!(empty[0].name, "frac", "空查询该先给最常用的模板");

        let frac = latex_completions_for("fra", 8);
        assert!(frac.iter().any(|entry| entry.name == "frac"));
        assert!(!frac.iter().any(|entry| entry.name == "alpha"));
        assert!(latex_completions_for("zzz", 8).is_empty());
    }

    #[test]
    fn command_before_cursor_extracts_letters_after_backslash() {
        assert_eq!(latex_command_before_cursor("x = \\fra"), Some((4, "fra")));
        assert_eq!(latex_command_before_cursor("x = \\"), Some((4, "")));
        assert_eq!(latex_command_before_cursor("x = frac"), None);
        assert_eq!(
            latex_command_before_cursor("\\\\alpha"),
            None,
            "双反斜杠是换行转义，不是命令"
        );
        // 命令名被空白截断后不算。
        assert_eq!(latex_command_before_cursor("\\alpha "), None);
    }

    #[test]
    fn inline_math_detection_counts_unescaped_dollars() {
        assert!(inside_inline_math("$x"));
        assert!(inside_inline_math("成本 $5 与 \\$x"));
        assert!(!inside_inline_math("价格 $5，运费 $6"));
        assert!(!inside_inline_math("没有公式"));
        assert!(!inside_inline_math("\\$ 单独一个转义的美元符"));
    }

    #[test]
    fn struct_commands_place_caret_inside_first_braces() {
        let frac = latex_completions_for("frac", 1).remove(0);
        assert_eq!(&frac.insert[..frac.caret], "\\frac{");
        assert_eq!(&frac.insert[frac.caret..], "}{}");

        let sum = latex_completions_for("sum", 1).remove(0);
        assert!(sum.insert[sum.caret..].starts_with("}^{}"));
    }

    #[test]
    fn plain_symbols_insert_the_command_with_trailing_space() {
        let alpha = latex_completions_for("alpha", 1).remove(0);
        assert_eq!(alpha.insert, "\\alpha ");
        assert_eq!(alpha.caret, "\\alpha ".len());
        assert_eq!(alpha.preview, "\\alpha");
    }

    #[test]
    fn every_panel_category_has_symbols() {
        // 公式面板六个页签各对应一组符号；某个分类为空就是一页空格子。
        for category in [
            LatexCategory::Structures,
            LatexCategory::Greek,
            LatexCategory::Operators,
            LatexCategory::Arrows,
            LatexCategory::Functions,
            LatexCategory::Symbols,
        ] {
            let count = LATEX_SYMBOLS
                .iter()
                .filter(|entry| entry.category == category)
                .count();
            assert!(count >= 4, "{category:?} 的符号少于 4 个: {count}");
        }
    }

    #[test]
    fn every_symbol_preview_renders() {
        // 表里每条的 preview 必须能被 ratex 渲染——补全列表与面板都靠它出图，
        // 坏一条就是界面上一个空格子。
        for entry in LATEX_SYMBOLS {
            let preview = if entry.preview.is_empty() {
                format!("\\{}", entry.name)
            } else {
                entry.preview.to_string()
            };
            assert!(
                super::super::render_latex_to_svg(
                    &preview,
                    gpui::Hsla::default(),
                    16.0,
                    super::super::MathLayout::Inline
                )
                .is_ok(),
                "\\{} 的预览片段应能渲染: {preview}",
                entry.name
            );
        }
    }
}
