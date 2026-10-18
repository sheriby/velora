use super::{PromptProfile, default_instruction};
use crate::ai::{AiCommand, settings::AiSettings};

#[test]
fn ai_instruction_catalog_localizes_all_defaults_and_falls_back_to_english() {
    for command in AiCommand::ALL
        .into_iter()
        .chain(std::iter::once(AiCommand::Ask))
    {
        let english = default_instruction(command, "en-US");
        let chinese = default_instruction(command, "zh-CN");
        assert!(!english.trim().is_empty() && !chinese.trim().is_empty());
        assert_ne!(
            english, chinese,
            "英文界面不能使用写死的中文指令：{command:?}"
        );
        assert_eq!(default_instruction(command, "fr-FR"), english);
    }
}

#[test]
fn ai_instruction_custom_body_keeps_the_review_and_edit_delivery_contract() {
    let mut settings = AiSettings::default();
    settings
        .quick_instructions
        .insert("polish".into(), "保留产品术语，表达自然。".into());
    let profile =
        PromptProfile::capture(AiCommand::Polish, "en-US", &settings, "- user:notes: Notes");
    let instruction = profile.quick_instruction.unwrap();
    assert!(
        instruction.contains("保留产品术语"),
        "内置指令应允许保存自己的要求"
    );
    assert!(
        instruction.contains("propose_edit"),
        "修改工具的交付要求不能依赖用户重复填写"
    );
    assert!(profile.system.contains("review") && profile.system.contains("user:notes"));
}
