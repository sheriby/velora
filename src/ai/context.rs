use anyhow::Result;

pub(crate) fn explicit_references(prompt: &str) -> Result<Vec<String>> {
    let expression = regex::Regex::new(r#"@(?:"([^"]+)"|([^\s，。；,;]+))"#)?;
    let mut references = Vec::new();
    for capture in expression.captures_iter(prompt) {
        if capture.get(0).is_some_and(|whole| {
            prompt
                .get(..whole.start())
                .and_then(|prefix| prefix.chars().next_back())
                .is_some_and(|previous| {
                    previous.is_ascii_alphanumeric() || matches!(previous, '_' | '.' | '+' | '-')
                })
        }) {
            continue;
        }
        if let Some(path) = capture.get(1).or_else(|| capture.get(2)) {
            let path = path.as_str().trim_end_matches('/').to_owned();
            if !path.is_empty() && !references.contains(&path) {
                references.push(path);
            }
        }
    }
    Ok(references)
}

#[cfg(test)]
mod tests;

pub(crate) fn expand_instruction(
    template: &str,
    selection: &str,
    document: &str,
    references: &[String],
) -> String {
    let mut result = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        result.push_str(remaining.get(..start).unwrap_or_default());
        let tail = remaining.get(start..).unwrap_or_default();
        let Some(end) = tail.find("}}") else {
            result.push_str(tail);
            return result;
        };
        let variable = tail.get(2..end).unwrap_or_default().trim();
        match variable {
            "selection" => result.push_str(selection),
            "document" => result.push_str(document),
            "references" => result.push_str(&references.join(", ")),
            _ => result.push_str(tail.get(..end + 2).unwrap_or_default()),
        }
        remaining = tail.get(end + 2..).unwrap_or_default();
    }
    result.push_str(remaining);
    result
}
