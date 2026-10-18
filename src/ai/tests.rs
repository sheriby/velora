use super::{ChangeSet, DocumentSnapshot};

fn document() -> DocumentSnapshot {
    DocumentSnapshot {
        id: "note.md".into(),
        source: "# 标题\n\n旧第一段。\n\n保留段。\n\n旧第二段。\n".into(),
    }
}

#[test]
fn ai_proposals_are_readable_before_accepting_without_changing_the_document() {
    let document = document();
    let original = document.source.clone();
    let mut changes = ChangeSet::default();
    assert!(
        changes
            .propose(document.clone(), "旧第一段。", "新第一段。")
            .is_ok()
    );
    assert!(changes.read(&document).contains("新第一段。"));
    assert_eq!(document.source, original, "提出修改不能写入正式正文");
    assert!(
        changes
            .propose(document.clone(), "新第一段。", "再次修订。")
            .is_ok()
    );
    assert!(changes.read(&document).contains("再次修订。"));
}

#[test]
fn ai_independent_edits_can_be_reviewed_separately_and_keep_unicode_offsets() {
    let document = document();
    let mut changes = ChangeSet::default();
    assert!(
        changes
            .propose(document.clone(), "旧第一段。", "新第一段。🙂")
            .is_ok()
    );
    assert!(
        changes
            .propose(document.clone(), "旧第二段。", "新第二段。")
            .is_ok()
    );
    let proposal = changes.proposals.get(&document.id).unwrap();
    let edits = proposal.edits();
    assert_eq!(edits.len(), 2);
    let mut accepted = document.source.to_string();
    for edit in edits.iter().rev() {
        accepted.replace_range(edit.range.clone(), &edit.replacement);
    }
    assert_eq!(accepted, changes.read(&document));
    changes.reject_edit(&document.id, 0).unwrap();
    assert!(changes.read(&document).contains("旧第一段。"));
    assert!(changes.read(&document).contains("新第二段。"));
}

#[test]
fn ai_proposals_refuse_ambiguous_matches_and_stale_document_versions() {
    let document = document();
    let mut changes = ChangeSet::default();
    assert!(
        changes
            .propose(document.clone(), "旧第一段。", "新第一段。")
            .is_ok()
    );
    let before = changes.read(&document).to_owned();
    assert!(changes.propose(document.clone(), "段。", "错误").is_err());
    assert_eq!(changes.read(&document), before);
    let mut updated = document.clone();
    updated.source = format!("{}用户继续写作", updated.source).into();
    assert!(
        changes
            .propose(updated.clone(), "新第一段。", "覆盖")
            .is_err()
    );
    let proposal = changes.proposals.get(&document.id).unwrap();
    assert!(proposal.validate(&updated.source).is_err());
}

#[test]
fn ai_accepting_one_edit_updates_the_base_without_accepting_other_changes() {
    let document = document();
    let mut changes = ChangeSet::default();
    changes
        .propose(document.clone(), "旧第一段。", "新第一段。")
        .unwrap();
    changes
        .propose(document.clone(), "旧第二段。", "新第二段。")
        .unwrap();
    let accepted = changes.accept_edit(&document.id, 0, &document.source);
    assert!(accepted.is_ok(), "{accepted:?}");
    let accepted = accepted.unwrap();
    let mut current = document.source.to_string();
    current.replace_range(accepted.range, &accepted.replacement);
    let proposal = &changes.proposals[&document.id];
    assert_eq!(proposal.base.source.as_ref(), current.as_str());
    assert_eq!(proposal.edits().len(), 1);
    assert!(proposal.proposed.contains("新第二段。"));
    assert!(
        changes
            .accept_edit(&document.id, 0, "用户改了原文")
            .is_err()
    );
}

#[test]
fn ai_small_changes_to_long_lines_do_not_duplicate_the_unchanged_line_in_edits() {
    let source = format!(
        "{}old-marker{}",
        "未修改的前文".repeat(5000),
        "未修改的后文".repeat(5000)
    );
    let document = DocumentSnapshot {
        id: "large.md".into(),
        source: source.clone().into(),
    };
    let mut changes = ChangeSet::default();
    changes
        .propose(document.clone(), "old-marker", "new-marker")
        .unwrap();
    let edits = changes.proposals[&document.id].edits();
    assert!(
        edits
            .iter()
            .map(|edit| edit.replacement.len())
            .sum::<usize>()
            < 1024,
        "局部修订不应把整条长行复制进应用和撤销记录"
    );
    let mut result = source;
    for edit in edits.iter().rev() {
        result.replace_range(edit.range.clone(), &edit.replacement);
    }
    assert_eq!(result, changes.read(&document));
}
