use super::{AiCommand, settings::AiSettings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PromptProfile {
    pub locale: String,
    pub command: AiCommand,
    pub system: String,
    pub quick_instruction: Option<String>,
}

pub(crate) fn default_instruction(command: AiCommand, locale: &str) -> &'static str {
    macro_rules! prompt {
        ($name:literal) => {
            if locale.starts_with("zh") {
                include_str!(concat!("../../resources/ai/prompts/zh-CN/", $name, ".md"))
            } else {
                include_str!(concat!("../../resources/ai/prompts/en-US/", $name, ".md"))
            }
            .trim()
        };
    }
    match command {
        AiCommand::Ask => prompt!("conversation"),
        AiCommand::Summarize => prompt!("summarize"),
        AiCommand::Explain => prompt!("explain"),
        AiCommand::Translate => prompt!("translate"),
        AiCommand::Polish => prompt!("polish"),
        AiCommand::Expand => prompt!("expand"),
    }
}

pub(crate) fn effective_instruction<'a>(
    settings: &'a AiSettings,
    command: AiCommand,
    locale: &str,
) -> &'a str {
    settings
        .quick_instructions
        .get(command.id())
        .filter(|body| !body.trim().is_empty())
        .map_or_else(|| default_instruction(command, locale), String::as_str)
}

impl PromptProfile {
    pub fn capture(command: AiCommand, locale: &str, settings: &AiSettings, skills: &str) -> Self {
        let (system, delivery) = if locale.starts_with("zh") {
            (
                include_str!("../../resources/ai/prompts/zh-CN/system.md"),
                include_str!("../../resources/ai/prompts/zh-CN/edit-delivery.md"),
            )
        } else {
            (
                include_str!("../../resources/ai/prompts/en-US/system.md"),
                include_str!("../../resources/ai/prompts/en-US/edit-delivery.md"),
            )
        };
        let system = format!(
            "{}\n\n<ui_language>{locale}</ui_language>\n\n<writing_preferences>\n{}\n</writing_preferences>\n\n<skills>\n{skills}\n</skills>",
            system.trim(),
            effective_instruction(settings, AiCommand::Ask, locale)
        );
        let quick_instruction = (command != AiCommand::Ask).then(|| {
            let body = effective_instruction(settings, command, locale);
            if command.edits_selection() {
                format!("{body}\n\n{}", delivery.trim())
            } else {
                body.to_owned()
            }
        });
        Self {
            locale: locale.into(),
            command,
            system,
            quick_instruction,
        }
    }
}

#[cfg(test)]
mod tests;
