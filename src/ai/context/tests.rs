use super::explicit_references;

#[test]
fn ai_context_mentions_support_quoted_paths_and_do_not_capture_email_addresses() {
    let references = explicit_references(
        "参考 @notes/资料.md 和 @\"项目 文档/会议记录\"，联系 person@example.com",
    )
    .unwrap();
    assert_eq!(references, vec!["notes/资料.md", "项目 文档/会议记录"]);
    assert!(
        explicit_references("邮件发给 person@example.com")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ai_context_templates_expand_values_once_and_keep_unknown_variables() {
    assert_eq!(
        super::expand_instruction(
            "校对 {{selection}}；来自 {{document}}，资料 {{references}}，{{unknown}}",
            "{{document}}",
            "笔记.md",
            &["参考.md".into()]
        ),
        "校对 {{document}}；来自 笔记.md，资料 参考.md，{{unknown}}"
    );
    assert_eq!(
        super::explicit_references("参考@文档.md 和 me@example.com").unwrap(),
        vec!["文档.md"]
    );
}
