    use super::super::Editor;
    use crate::components::{
        BlockKind, InlineTextTree,
    };
use std::path::{Path, PathBuf};
    use gpui::{AppContext, TestAppContext};

    #[test]
    fn untitled_workspace_image_uses_the_workspace_root() {
        assert_eq!(
            Editor::image_paste_base_dir(None, Some(Path::new("/workspace"))),
            Some(PathBuf::from("/workspace"))
        );
        assert_eq!(
            Editor::image_paste_base_dir(
                Some(Path::new("/workspace/docs/readme.md")),
                Some(Path::new("/workspace")),
            ),
            Some(PathBuf::from("/workspace/docs"))
        );
    }

    #[test]
    fn pasted_image_hash_is_stable_and_content_sensitive() {
        let first = Editor::pasted_image_hash(b"image-bytes-a");
        let second = Editor::pasted_image_hash(b"image-bytes-a");
        let other = Editor::pasted_image_hash(b"image-bytes-b");

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(first.len(), 8);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn clipboard_image_name_uses_date_and_hash_template() {
        let name = format!(
            "{}-{}.png",
            crate::config::today_local_date(),
            Editor::pasted_image_hash(b"png-bytes")
        );
        let (stem, extension) = name.split_once('.').expect("extension");
        assert_eq!(extension, "png");
        let mut parts = stem.splitn(3, '-');
        let year = parts.next().expect("year");
        let month = parts.next().expect("month");
        let rest = parts.next().expect("day+hash");
        let (day, hash) = rest.split_once('-').expect("day and hash");
        assert_eq!(year.len(), 4);
        assert_eq!(month.len(), 2);
        assert_eq!(day.len(), 2);
        assert_eq!(hash.len(), 8);
        assert!(
            year.chars()
                .chain(month.chars())
                .chain(day.chars())
                .all(|ch| ch.is_ascii_digit())
        );
    }

    #[gpui::test]
    async fn image_block_insert_preserves_surrounding_paragraph_text(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "beforeafter".to_string(), None));

        editor.update(cx, |editor, cx| {
            let paragraph = editor.document.first_root().expect("paragraph").clone();
            editor.insert_image_block_after_paragraph(
                &paragraph,
                &InlineTextTree::plain("before"),
                "![image](./assets/image.png)",
                &InlineTextTree::plain("after"),
                cx,
            );

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 3);
            assert_eq!(visible[0].entity.read(cx).display_text(), "before");
            assert_eq!(
                visible[1].entity.read(cx).display_text(),
                "![image](./assets/image.png)"
            );
            assert!(visible[1].entity.read(cx).image_runtime().is_some());
            assert_eq!(visible[2].entity.read(cx).display_text(), "after");
        });
    }

    #[gpui::test]
    async fn image_paste_text_in_code_block_stays_inside_block(cx: &mut TestAppContext) {
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "```\nbeforeafter\n```".to_string(), None));

        editor.update(cx, |editor, cx| {
            let block = editor.document.first_root().expect("code block").clone();
            editor.replace_current_block_selection_with_image_text(
                &block,
                &InlineTextTree::plain("before"),
                "![image](./assets/image.png)",
                &InlineTextTree::plain("after"),
                cx,
            );

            let visible = editor.document.visible_blocks();
            assert_eq!(visible.len(), 1);
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::CodeBlock { language: None }
            );
            assert_eq!(
                visible[0].entity.read(cx).display_text(),
                "before![image](./assets/image.png)after"
            );
        });
    }

