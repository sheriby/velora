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

    /// 目的地带空格时整体包 `<>`：裸目的地按 CommonMark 不许含空格，导出侧的
    /// pulldown（严格 CommonMark）会断在空格处把整张图当字面文本。
    #[test]
    fn image_target_with_spaces_wraps_in_angle_brackets() {
        assert_eq!(
            Editor::markdown_image_target("./screen shot 1.png"),
            "<./screen shot 1.png>"
        );
        // 无空格的路径维持原口径：只转义 `\ ( "`，不额外包尖括号。
        assert_eq!(
            Editor::markdown_image_target("./assets/图pic(1).png"),
            "./assets/图pic\\(1\\).png"
        );
    }

    /// 带空格文件名粘贴后的完整口径：应用内解析（渲染端）与导出侧 pulldown
    /// 都要把它认回同一张图。
    #[test]
    fn pasted_image_markdown_with_spaces_parses_in_app_and_in_export() {
        use crate::components::markdown::image::{ImageTarget, parse_standalone_image};
        use pulldown_cmark::{Event, Options, Parser, Tag};

        let markdown = format!(
            "![shot]({})",
            Editor::markdown_image_target("./screen shot 1.png")
        );

        let syntax = parse_standalone_image(&markdown).expect("应用内应认成图片");
        assert_eq!(
            syntax.target,
            ImageTarget::Direct {
                src: "./screen shot 1.png".to_string(),
                title: None,
            },
            "应用内解析应剥掉尖括号拿回带空格的原始路径"
        );

        let image_destinations = Parser::new_ext(&markdown, Options::all())
            .filter_map(|event| match event {
                Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            image_destinations,
            vec!["./screen shot 1.png".to_string()],
            "导出侧 pulldown（严格 CommonMark）应把尖括号目的地整体认作图片地址"
        );
    }

    /// 探测有上界：目标目录里已存在一整套按序号命名的资产目录时，报错而不是
    /// 无限探测（旧实现是 `for index in 0..` + `unreachable!`）。
    #[gpui::test]
    async fn image_target_dir_gives_up_after_bounded_probes(cx: &mut TestAppContext) {
        let root = std::env::temp_dir().join(format!(
            "velora-image-target-probe-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("probe root should be created");
        for index in 0..=1000 {
            let folder = if index == 0 {
                "untitle.assets".to_string()
            } else {
                format!("untitle{index}.assets")
            };
            std::fs::create_dir_all(root.join(&folder)).expect("probe folder should be created");
        }

        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, _cx| {
            let result = Editor::image_target_dir_for(
                crate::config::ImagePasteBehavior::CopyToNamedAssetsFolder,
                &root,
                None,
                &crate::components::PastedImageSource::ClipboardImage(gpui::Image::from_bytes(
                    gpui::ImageFormat::Png,
                    Vec::new(),
                )),
            );
            assert!(result.is_err(), "名字空间占满时应报错而不是继续探测");
        });

        std::fs::remove_dir_all(&root).expect("probe root should be removed");
    }

    /// 没有文档路径也没有工作区根时，粘贴图片不落进进程当前目录：给提示，
    /// 让用户先保存文档或打开工作区（以前退回 CWD，文件随启动方式漂移，
    /// 文档一换位置引用全断）。
    #[gpui::test]
    async fn untitled_document_without_workspace_does_not_write_images_into_cwd(
        cx: &mut TestAppContext,
    ) {
        // 隔离用户偏好：默认资产目录是 `assets/`，测试不能在仓库根下落文件。
        let config_root = std::env::temp_dir().join(format!(
            "velora-image-cwd-config-{}",
            uuid::Uuid::new_v4()
        ));
        let _config = crate::config::override_test_config_root(&config_root);
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "正文".to_string(), None)
        });
        let bytes = b"pasted-image-bytes".to_vec();
        let file_name = format!(
            "{}-{}.png",
            crate::config::today_local_date(),
            Editor::pasted_image_hash(&bytes)
        );
        let cwd = std::env::current_dir().expect("cwd");
        let cwd_targets = [cwd.join(&file_name), cwd.join("assets").join(&file_name)];
        for target in &cwd_targets {
            let _ = std::fs::remove_file(target);
        }
        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.root_blocks()[0].clone();
                editor.on_block_event(
                    block,
                    &crate::components::BlockEvent::RequestPasteImage {
                        leading: InlineTextTree::plain("正".to_string()),
                        source: crate::components::PastedImageSource::ClipboardImage(
                            gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.clone()),
                        ),
                        trailing: InlineTextTree::plain("文".to_string()),
                    },
                    cx,
                );
            });
        });
        for target in &cwd_targets {
            assert!(
                !target.exists(),
                "图片被写进了进程当前目录：{}",
                target.display()
            );
        }
        let title = cx.update(|_window, cx| {
            cx.global::<crate::i18n::I18nManager>()
                .strings()
                .image_paste_failed_title
                .clone()
        });
        editor.read_with(cx, |editor, _cx| {
            let spec = editor.modal_spec().expect("没有落脚点要有提示");
            assert_eq!(spec.title, title);
        });
    }

    /// 粘贴图片的复制/写盘不在调用线程上做完：受理返回时目标文件还不存在，
    /// 后台一轮之后才落盘并插入引用。
    #[gpui::test]
    async fn pasted_image_is_copied_to_disk_after_the_background_round(
        cx: &mut TestAppContext,
    ) {
        let config_root = std::env::temp_dir().join(format!(
            "velora-image-paste-config-{}",
            uuid::Uuid::new_v4()
        ));
        let _config = crate::config::override_test_config_root(&config_root);
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let workdir = std::env::temp_dir().join(format!(
            "velora-image-paste-doc-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workdir).expect("workdir");
        let document_path = workdir.join("note.md");
        std::fs::write(&document_path, "正文\n").expect("document");
        let source_path = std::env::temp_dir().join(format!(
            "velora-image-paste-source-{}.png",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&source_path, b"png-bytes").expect("source image");
        let (editor, cx) = cx.add_window_view({
            let document_path = document_path.clone();
            move |_window, cx| {
                Editor::from_markdown(cx, "正文\n".to_string(), Some(document_path))
            }
        });
        cx.update(|_window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.root_blocks()[0].clone();
                editor.on_block_event(
                    block,
                    &crate::components::BlockEvent::RequestPasteImage {
                        leading: InlineTextTree::plain("正".to_string()),
                        source: crate::components::PastedImageSource::LocalPath(
                            source_path.clone(),
                        ),
                        trailing: InlineTextTree::plain("文".to_string()),
                    },
                    cx,
                );
            });
        });
        let target = workdir
            .join("assets")
            .join(source_path.file_name().expect("file name"));
        assert!(
            !target.exists(),
            "受理返回时图片已经复制完成：物化仍在调用线程上同步执行"
        );
        cx.run_until_parked();
        assert!(target.exists(), "后台一轮之后图片应落盘");
        editor.read_with(cx, |editor, cx| {
            assert!(
                editor.document.markdown_text(cx).contains("![velora-image-paste-source"),
                "图片引用没写进文档"
            );
        });
        let _ = std::fs::remove_dir_all(&workdir);
        let _ = std::fs::remove_file(&source_path);
    }
