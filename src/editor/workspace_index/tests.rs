mod tests {
    use super::super::*;

    // 仅供测试断言使用（生产路径不读取文件数），故收进测试构建。
    impl WorkspaceLinkIndex {
        pub(crate) fn tracked_file_count(&self) -> usize {
            self.entries.len()
        }
    }

    #[test]
    fn extracts_wikilinks_tags_and_skips_code() {
        let source = "\
# 笔记 #top\n\n链接到 [[另一个笔记]] 和 [[带空格 的目标]]，还有 [[\n\n```rust\nlet s = \"[[不是链接]]\"; // #nottag\n```\n\n行内代码 `[[nope]] #nope` 之后的 [[真链接]] 和 #tag-1、#tag_2。\n";
        let entry = extract_links_and_tags(source);
        assert_eq!(
            entry.wikilinks,
            vec!["另一个笔记", "带空格 的目标", "真链接"],
            "围栏与行内代码里的 [[..]] 不应计入"
        );
        assert_eq!(
            entry.tags,
            vec!["#top", "#tag-1", "#tag_2"],
            "标签格式与 C4 tag_query 一致，代码里的 #nottag 不计入"
        );
    }

    #[test]
    fn fence_toggles_require_matching_marker() {
        let source = "~~~\n[[nope]]\n```\n[[still-nope]]\n~~~\n[[yes]]";
        let entry = extract_links_and_tags(source);
        assert_eq!(entry.wikilinks, vec!["yes"]);
    }

    #[test]
    fn backlinks_match_stem_or_filename_case_insensitive() {
        let mut index = WorkspaceLinkIndex::default();
        index.entries.insert(
            PathBuf::from("/ws/notes/a.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["B".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/b.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["a.md".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/c.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["无关于目标".into()],
                tags: vec![],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/notes/deleted.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec!["b".into()],
                tags: vec![],
            }),
        );
        let target = PathBuf::from("/ws/notes/b.md");
        let live = vec![
            PathBuf::from("/ws/notes/a.md"),
            PathBuf::from("/ws/notes/b.md"),
            PathBuf::from("/ws/notes/c.md"),
        ];
        let backlinks = index.backlinks_to(&target, &live, None);
        // a 指到 stem「b」；b 自指被排除；c 无关；deleted 已不在文件树。
        assert_eq!(backlinks, vec![PathBuf::from("/ws/notes/a.md")]);
    }

    #[test]
    fn active_document_overlay_supplies_unsaved_links_without_duplicates() {
        let mut index = WorkspaceLinkIndex::default();
        let target = PathBuf::from("/ws/notes/b.md");
        let live = vec![
            PathBuf::from("/ws/notes/a.md"),
            PathBuf::from("/ws/notes/b.md"),
        ];
        // 磁盘上的活动文档还没有链接。
        index.entries.insert(
            PathBuf::from("/ws/notes/a.md"),
            Arc::new(FileLinkEntry::default()),
        );
        // 未保存的编辑加了 [[b]]。
        let overlay_entry = extract_links_and_tags("看 [[b]] 和 [[b]]");
        let backlinks = index.backlinks_to(
            &target,
            &live,
            Some((
                PathBuf::from("/ws/notes/a.md"),
                7,
                overlay_entry,
            )),
        );
        assert_eq!(backlinks, vec![PathBuf::from("/ws/notes/a.md")]);

        // revision 变了覆盖层才重算：再查一次走缓存路径也不重复。
        let again = index.backlinks_to(&target, &live, None);
        assert!(again.is_empty(), "无覆盖层时磁盘上没有反链");
    }

    #[test]
    fn tag_counts_dedupe_within_file_and_rank_by_count() {
        let mut index = WorkspaceLinkIndex::default();
        index.entries.insert(
            PathBuf::from("/ws/a.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec![],
                tags: vec!["#rust".into(), "#rust".into(), "#gpui".into()],
            }),
        );
        index.entries.insert(
            PathBuf::from("/ws/b.md"),
            Arc::new(FileLinkEntry {
                wikilinks: vec![],
                tags: vec!["#rust".into()],
            }),
        );
        let live = vec![PathBuf::from("/ws/a.md"), PathBuf::from("/ws/b.md")];
        let counts = index.tag_counts(&live, None);
        assert_eq!(
            counts,
            vec![("#rust".to_string(), 2), ("#gpui".to_string(), 1)],
            "同文件内重复标签只计一次，按引用文件数降序"
        );
    }
}
