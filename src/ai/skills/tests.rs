use super::*;

#[test]
fn ai_skills_preserve_scope_and_read_references_only_when_requested() {
    let root = std::env::temp_dir().join(format!("velora-ai-skills-{}", uuid::Uuid::new_v4()));
    for scope in ["user", "workspace"] {
        let directory = root.join(scope).join("review");
        std::fs::create_dir_all(directory.join("references")).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            "---\nname: review\ndescription: 校对技术文章\n---\n\n先检查术语。\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("references/style.md"),
            "仅在请求时读取的参考资料",
        )
        .unwrap();
    }
    let catalog = SkillCatalog::load(&[
        (root.join("user"), "user"),
        (root.join("workspace"), "workspace"),
    ]);
    assert_eq!(catalog.entries.len(), 2, "同名任务不能静默覆盖");
    assert!(catalog.warnings.is_empty(), "{:?}", catalog.warnings);
    assert_eq!(
        catalog
            .read("workspace:review", "references/style.md")
            .unwrap(),
        "仅在请求时读取的参考资料"
    );
    assert!(
        catalog
            .read("workspace:review", "../../user/review/SKILL.md")
            .is_err()
    );
    assert!(catalog.read("user:review", "/etc/passwd").is_err());
    let enabled = catalog.enabled(&["user:review".into()]);
    assert_eq!(enabled.entries.len(), 1);
    assert!(enabled.read("user:review", "SKILL.md").is_err());
    assert!(enabled.prompt_catalog().contains("workspace:review"));
    std::fs::remove_dir_all(root).unwrap();
}
