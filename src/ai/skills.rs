//! 用户指令与按需加载的任务说明。

use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct SkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub directory: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SkillCatalog {
    pub entries: Vec<SkillEntry>,
    pub warnings: Vec<String>,
}

impl SkillCatalog {
    pub fn load(directories: &[(PathBuf, &str)]) -> Self {
        let mut catalog = Self::default();
        for (directory, scope) in directories {
            if !directory.exists() {
                continue;
            }
            let (skills, errors) = yoagent::SkillSet::load_dir_resilient(directory, scope);
            catalog
                .warnings
                .extend(errors.into_iter().map(|error| error.to_string()));
            for skill in skills.skills() {
                match skill.base_dir.canonicalize() {
                    Ok(directory) => catalog.entries.push(SkillEntry {
                        id: format!("{scope}:{}", skill.name),
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        directory,
                    }),
                    Err(error) => catalog.warnings.push(error.to_string()),
                }
            }
        }
        catalog
    }

    pub fn enabled(&self, disabled: &[String]) -> Self {
        let mut catalog = self.clone();
        catalog
            .entries
            .retain(|skill| !disabled.contains(&skill.id));
        catalog
    }

    pub fn read(&self, id: &str, resource: &str) -> Result<String> {
        let skill = self
            .entries
            .iter()
            .find(|skill| skill.id == id)
            .context("任务说明不存在")?;
        let resource = Path::new(resource);
        if resource
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || resource.as_os_str().is_empty()
        {
            bail!("只能读取任务目录中的相对资料路径");
        }
        let path = skill.directory.join(resource).canonicalize()?;
        if !path.starts_with(&skill.directory) {
            bail!("资料路径超出任务目录");
        }
        if path.metadata()?.len() > 64 * 1024 {
            bail!("参考资料超过单次读取预算");
        }
        Ok(std::fs::read_to_string(path)?)
    }

    pub fn active_skill_references(
        &self,
        disabled: &[String],
        prompt: &str,
    ) -> std::result::Result<Vec<String>, String> {
        let mut active = Vec::new();
        for token in prompt.split_whitespace() {
            let Some(id) = token
                .trim_matches(|character: char| {
                    matches!(character, '，' | '。' | '；' | ';' | ',' | '.' | '!' | '？' | '?')
                })
                .strip_prefix('/')
            else {
                continue;
            };
            // 只把 `/scope:name` 形态的 token 当任务说明引用，普通路径与斜杠
            // 短语不应被拦截。
            if !id.contains(':') {
                continue;
            }
            if !self.entries.iter().any(|entry| entry.id == id) {
                return Err(format!("未知的任务说明：{id}"));
            }
            if disabled.iter().any(|entry| entry == id) {
                return Err(format!("任务说明已禁用：{id}"));
            }
            if !active.iter().any(|existing| existing == id) {
                active.push(id.to_owned());
            }
        }
        Ok(active)
    }

    pub fn prompt_catalog(&self) -> String {
        let mut entries = self.entries.iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        entries
            .into_iter()
            .map(|skill| format!("- {}: {} ({})", skill.id, skill.description, skill.name))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests;
