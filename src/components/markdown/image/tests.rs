mod tests {
    use super::super::{
        ImageReferenceDefinition, ImageResolvedSource, ImageSyntax, ImageTarget,
        TableCellInlineImageSegment, normalize_reference_label, parse_image_reference_definitions,
        parse_standalone_image, parse_table_cell_inline_images, resolve_image_source,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    /// 只用来占位：阅读视图的解析断言只看路径，不看像素。
    const PNG_FIXTURE: [u8; 8] = [0x89, b'P', b'N', b'G', 1, 2, 3, 4];

    /// 造一个 `assets/` 目录，里面放三种真实文件名（含空格、含字面 `%`、含中文）。
    fn fixture_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!("velora-image-resolve-{}", Uuid::new_v4()));
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("create assets dir");
        for name in ["blue card.png", "100% done.png", "封面.png"] {
            fs::write(assets.join(name), PNG_FIXTURE).expect("write fixture image");
        }
        root
    }

    fn local_path_of(source: &str, base_dir: Option<&Path>) -> PathBuf {
        match resolve_image_source(source, base_dir) {
            ImageResolvedSource::Local(path) => path,
            other => panic!("应解析成本地路径，实际 {other:?}（写法 {source:?}）"),
        }
    }

    #[test]
    fn parses_standalone_image_without_title() {
        let parsed = parse_standalone_image("![alt](./img.png)").expect("image syntax");
        assert_eq!(parsed.alt, "alt");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./img.png".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_surrounding_whitespace() {
        let three_space =
            parse_standalone_image("   ![alt](https://example.com/a.png)").expect("image syntax");
        assert_eq!(three_space.alt, "alt");
        assert_eq!(
            three_space.target,
            ImageTarget::Direct {
                src: "https://example.com/a.png".to_string(),
                title: None,
            }
        );

        let deeply_indented =
            parse_standalone_image("        ![alt](https://example.com/a.png)   ")
                .expect("image syntax");
        assert_eq!(deeply_indented, three_space);
        assert!(parse_standalone_image("   text ![alt](x)").is_none());
        assert!(parse_standalone_image("   ![alt](x)\n").is_none());
    }

    #[test]
    fn parses_image_target_with_escaped_punctuation_in_source() {
        let parsed = parse_standalone_image("![alt](https://example.com/typera\\_picgo/img.png)")
            .expect("image syntax");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "https://example.com/typera_picgo/img.png".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_underscores_in_alt_and_source() {
        let parsed = parse_standalone_image(
            "![1.1_进制转换例子](./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg)",
        )
        .expect("image syntax");

        assert_eq!(parsed.alt, "1.1_进制转换例子");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn parses_standalone_image_with_title() {
        let parsed =
            parse_standalone_image("![alt](./img.png \"caption text\")").expect("image syntax");
        assert_eq!(parsed.alt, "alt");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./img.png".to_string(),
                title: Some("caption text".to_string()),
            }
        );
    }

    #[test]
    fn parses_reference_style_standalone_image() {
        let parsed =
            parse_standalone_image("![reference image][ref-image]").expect("reference image");
        assert_eq!(parsed.alt, "reference image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "ref-image".to_string(),
            }
        );
    }

    #[test]
    fn parses_collapsed_reference_style_standalone_image() {
        let parsed =
            parse_standalone_image("![collapsed image][]").expect("collapsed reference image");
        assert_eq!(parsed.alt, "collapsed image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "collapsed image".to_string(),
            }
        );
    }

    #[test]
    fn parses_shortcut_reference_style_standalone_image() {
        let parsed = parse_standalone_image("![shortcut image]").expect("shortcut reference image");
        assert_eq!(parsed.alt, "shortcut image");
        assert_eq!(
            parsed.target,
            ImageTarget::Reference {
                label: "shortcut image".to_string(),
            }
        );
    }

    #[test]
    fn rejects_mixed_or_wrapped_image_syntax() {
        assert!(parse_standalone_image("text ![alt](./img.png)").is_none());
        assert!(parse_standalone_image("[![alt](./img.png)](https://example.com)").is_none());
        assert!(parse_standalone_image("![][]").is_none());
        assert!(parse_standalone_image("![]").is_none());
    }

    #[test]
    fn parses_image_with_trailing_width_attribute() {
        let parsed = parse_standalone_image("![alt](./img.png){width=60%}").expect("image syntax");
        assert_eq!(parsed.alt, "alt");
        assert_eq!(
            parsed.target,
            ImageTarget::Direct {
                src: "./img.png".to_string(),
                title: None,
            }
        );
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png){width=60%}"),
            Some(60)
        );
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png)"),
            None
        );
    }

    #[test]
    fn rejects_malformed_width_attribute() {
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png){width=0%}"),
            None
        );
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png){width=101%}"),
            None
        );
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png){width=abc%}"),
            None
        );
        assert_eq!(
            super::super::standalone_image_width_percent("![alt](./img.png){height=60%}"),
            None
        );
    }

    #[test]
    fn table_cell_inline_images_ignore_inline_code_content() {
        // 行内代码内部是字面文本：`![alt](path)` 不能被提升为图片段
        let only_code = parse_table_cell_inline_images("源码 `![alt](path){width=NN%}`，100%");
        assert_eq!(
            only_code,
            vec![TableCellInlineImageSegment::Text(
                "源码 `![alt](path){width=NN%}`，100%".to_string()
            )],
        );

        // 代码段之外的图片照常提升为图片段
        let segments = parse_table_cell_inline_images("`![a](x.png)` 与 ![b](y.png)");
        assert!(
            segments.iter().any(|segment| matches!(
                segment,
                TableCellInlineImageSegment::Text(text) if text.contains("`![a](x.png)`")
            )),
            "代码段内的图片语法应留在文本段里: {segments:?}"
        );
        assert!(matches!(
            segments.last(),
            Some(TableCellInlineImageSegment::Image { .. })
        ));
    }

    #[test]
    fn parses_table_cell_inline_image_segments() {
        let segments = parse_table_cell_inline_images("image ![alt](https://example.com/x.png)");
        assert_eq!(
            segments,
            vec![
                TableCellInlineImageSegment::Text("image ".to_string()),
                TableCellInlineImageSegment::Image {
                    markdown: "![alt](https://example.com/x.png)".to_string(),
                    syntax: ImageSyntax {
                        alt: "alt".to_string(),
                        target: ImageTarget::Direct {
                            src: "https://example.com/x.png".to_string(),
                            title: None,
                        },
                    },
                },
            ]
        );
    }

    #[test]
    fn parses_multiple_table_cell_inline_images() {
        let segments = parse_table_cell_inline_images("![a](x.png) and ![b](y.png)");
        assert_eq!(segments.len(), 3);
        assert!(matches!(
            &segments[0],
            TableCellInlineImageSegment::Image { syntax, .. } if syntax.alt == "a"
        ));
        assert_eq!(
            segments[1],
            TableCellInlineImageSegment::Text(" and ".to_string())
        );
        assert!(matches!(
            &segments[2],
            TableCellInlineImageSegment::Image { syntax, .. } if syntax.alt == "b"
        ));
    }

    #[test]
    fn table_cell_inline_image_segments_keep_escaped_wrapped_and_broken_text() {
        assert_eq!(
            parse_table_cell_inline_images(r"\![alt](x.png)"),
            vec![TableCellInlineImageSegment::Text(
                r"\![alt](x.png)".to_string()
            )]
        );
        assert_eq!(
            parse_table_cell_inline_images("[![alt](x.png)](https://example.com)"),
            vec![TableCellInlineImageSegment::Text(
                "[![alt](x.png)](https://example.com)".to_string()
            )]
        );
        assert_eq!(
            parse_table_cell_inline_images("broken ![alt](x.png"),
            vec![TableCellInlineImageSegment::Text(
                "broken ![alt](x.png".to_string()
            )]
        );
    }

    #[test]
    fn table_cell_inline_reference_images_resolve() {
        let definitions = parse_image_reference_definitions(
            "[ref]: ./ref.png\n[collapsed]: ./collapsed.png\n[shortcut]: ./shortcut.png",
        );
        let segments = parse_table_cell_inline_images("![full][ref] ![collapsed][] ![shortcut]");
        let resolved = segments
            .iter()
            .filter_map(|segment| match segment {
                TableCellInlineImageSegment::Image { syntax, .. } => {
                    syntax.resolve_target(&definitions).map(|target| target.src)
                }
                TableCellInlineImageSegment::Text(_) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            resolved,
            vec!["./ref.png", "./collapsed.png", "./shortcut.png"]
        );
    }

    #[test]
    fn parses_image_reference_definitions_with_title_and_first_wins() {
        let definitions = parse_image_reference_definitions(
            "[Ref Image]: ./first.png \"Caption\"\n[ref image]: ./second.png".trim(),
        );
        assert_eq!(
            definitions.get("ref image"),
            Some(&ImageReferenceDefinition {
                src: "./first.png".to_string(),
                title: Some("Caption".to_string()),
            })
        );
    }

    #[test]
    fn normalizes_reference_labels_case_and_whitespace_insensitively() {
        assert_eq!(
            normalize_reference_label("  Ref\t Image  "),
            Some("ref image".to_string())
        );
    }

    #[test]
    fn resolves_reference_targets() {
        let syntax = ImageSyntax {
            alt: "alt".to_string(),
            target: ImageTarget::Reference {
                label: "ref-image".to_string(),
            },
        };
        let definitions = parse_image_reference_definitions("[ref-image]: ./img.png \"Caption\"");
        let resolved = syntax
            .resolve_target(&definitions)
            .expect("resolved target");
        assert_eq!(resolved.src, "./img.png");
        assert_eq!(resolved.title.as_deref(), Some("Caption"));
    }

    #[test]
    fn resolves_collapsed_and_shortcut_reference_images() {
        let definitions = parse_image_reference_definitions(
            "[collapsed image]: ./collapsed.png\n[shortcut image]: ./shortcut.png",
        );

        let collapsed = parse_standalone_image("![collapsed image][]")
            .expect("collapsed reference image")
            .resolve_target(&definitions)
            .expect("resolved collapsed image");
        assert_eq!(collapsed.src, "./collapsed.png");

        let shortcut = parse_standalone_image("![shortcut image]")
            .expect("shortcut reference image")
            .resolve_target(&definitions)
            .expect("resolved shortcut image");
        assert_eq!(shortcut.src, "./shortcut.png");
    }

    #[test]
    fn unresolved_reference_target_returns_none() {
        let syntax = ImageSyntax {
            alt: "alt".to_string(),
            target: ImageTarget::Reference {
                label: "missing".to_string(),
            },
        };
        assert!(
            syntax
                .resolve_target(&parse_image_reference_definitions("[ref]: ./img.png"))
                .is_none()
        );
    }

    #[test]
    fn resolves_relative_and_remote_sources() {
        let local = resolve_image_source("images/pic.png", Some(Path::new("D:/docs")));
        assert_eq!(
            local,
            ImageResolvedSource::Local(Path::new("D:/docs").join("images/pic.png"))
        );

        let remote = resolve_image_source("https://example.com/img.gif", None);
        match remote {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/img.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    }

    #[test]
    fn resolves_every_local_destination_spelling_to_the_same_file() {
        // cases/12-images.md：同一个文件的三种写法必须解析到同一个路径。阅读视图
        // 原先只认 `%20`，`<assets/blue card.png>` 因为「空格不是合法 URI 字符」
        // 没剥尖括号，落在一个不存在的文件上——图片在正文里显示成加载失败。
        let root = fixture_dir();
        let assets = root.join("assets");
        let blue = assets.join("blue card.png");
        let file_url = url::Url::from_file_path(&blue)
            .expect("temp image path should form file URL")
            .to_string();

        let cases: Vec<(&str, &str, PathBuf)> = vec![
            ("直接路径含空格", "assets/blue card.png", blue.clone()),
            ("percent 空格", "assets/blue%20card.png", blue.clone()),
            ("尖括号含空格", "<assets/blue card.png>", blue.clone()),
            ("尖括号加 percent", "<assets/blue%20card.png>", blue.clone()),
            ("file URL", file_url.as_str(), blue.clone()),
            ("字面 percent 写 %25", "assets/100%25 done.png", assets.join("100% done.png")),
            ("中文文件名", "assets/封面.png", assets.join("封面.png")),
            (
                "中文 percent 转义",
                "assets/%E5%B0%81%E9%9D%A2.png",
                assets.join("封面.png"),
            ),
        ];
        for (label, spelling, expected) in cases {
            let resolved = local_path_of(spelling, Some(&root));
            assert!(
                resolved.is_file(),
                "{label}：{spelling:?} 解析成 {resolved:?}，不是存在的文件（期望 {expected:?}）"
            );
            assert_eq!(resolved, expected, "{label}：{spelling:?}");
        }

        fs::remove_dir_all(&root).expect("clean up fixture dir");
    }

    #[test]
    fn resolves_angle_bracket_destination_that_is_not_a_valid_uri() {
        // 尖括号是 Markdown 语法不是 URI 语法：内部允许空格，只不许出现未转义的
        // `<`/`>` 和换行。原先用 `Uri::from_str` 判定，带空格的路径判不过。
        let spaced = parse_standalone_image("![blue angle](<assets/blue card.png>)")
            .expect("angle bracketed image");
        assert_eq!(
            spaced.target,
            ImageTarget::Direct {
                src: "assets/blue card.png".to_string(),
                title: None,
            }
        );

        let with_title = parse_standalone_image("![blue angle](<assets/blue card.png> \"Blue\")")
            .expect("angle bracketed image with title");
        assert_eq!(
            with_title.target,
            ImageTarget::Direct {
                src: "assets/blue card.png".to_string(),
                title: Some("Blue".to_string()),
            }
        );

        let escaped =
            parse_standalone_image("![blue angle](<assets/blue\\>card.png>)").expect("escaped");
        assert_eq!(
            escaped.target,
            ImageTarget::Direct {
                src: "assets/blue>card.png".to_string(),
                title: None,
            }
        );

        // 未转义的 `>` 结束不了尖括号目标：原样留下，交给文件系统判定存在与否。
        let unescaped = parse_standalone_image("![blue angle](<assets/blue>card.png>)")
            .expect("broken angle bracketed image");
        assert_eq!(
            unescaped.target,
            ImageTarget::Direct {
                src: "<assets/blue>card.png>".to_string(),
                title: None,
            }
        );
    }

    #[test]
    fn resolves_relative_file_url_inside_the_document_directory() {
        // `file:` 后面跟相对写法不是绝对 URL：不能交给 URL 语法解成根目录下的文件，
        // 要按文档目录解析（报告 12 的根因就是每种写法各有各的解析处）。
        let root = fixture_dir();
        assert_eq!(
            local_path_of("file:relative.png", Some(&root)),
            root.join("relative.png")
        );
        fs::remove_dir_all(&root).expect("clean up fixture dir");
    }

    #[test]
    fn keeps_remote_sources_remote_and_out_of_the_filesystem() {
        // 远程写法不能被 percent 解码后当本地文件读：`%20` 是 URL 的一部分，
        // 请求时要原样送出。
        for uri in [
            "https://example.com/blue%20card.png",
            "http://example.com/assets/封面.png",
        ] {
            match resolve_image_source(uri, Some(Path::new("/tmp/does-not-matter"))) {
                ImageResolvedSource::Remote(resolved) => {
                    assert_eq!(resolved.to_string(), uri, "远程写法被改写了：{uri}");
                }
                other => panic!("远程写法不应落到本地路径：{uri} -> {other:?}"),
            }
        }
    }

    #[test]
    fn resolves_reference_definition_destinations_with_the_same_rule() {
        // 引用式定义与内联写法共用同一口径，否则 `[ref]: <assets/blue card.png>`
        // 在正文里能显示、在引用表里就 404。
        let definitions = parse_image_reference_definitions("[blue]: <assets/blue card.png>");
        let syntax = ImageSyntax {
            alt: "blue".to_string(),
            target: ImageTarget::Reference {
                label: "blue".to_string(),
            },
        };
        let target = syntax
            .resolve_target(&definitions)
            .expect("reference target");
        let root = fixture_dir();
        assert_eq!(
            local_path_of(&target.src, Some(&root)),
            root.join("assets").join("blue card.png")
        );
        fs::remove_dir_all(&root).expect("clean up fixture dir");
    }

    #[test]
    fn parses_container_scoped_reference_definitions_in_source_order() {
        let definitions = parse_image_reference_definitions(
            [
                "> [quoted ref]: ./quoted.png \"Quoted\"",
                "- [list ref]: ./list.png",
                "1) [ordered ref]: ./ordered.png",
                "> > [quoted ref]: ./ignored.png",
            ]
            .join("\n")
            .as_str(),
        );

        assert_eq!(
            definitions.get("quoted ref"),
            Some(&ImageReferenceDefinition {
                src: "./quoted.png".to_string(),
                title: Some("Quoted".to_string()),
            })
        );
        assert_eq!(
            definitions.get("list ref"),
            Some(&ImageReferenceDefinition {
                src: "./list.png".to_string(),
                title: None,
            })
        );
        assert_eq!(
            definitions.get("ordered ref"),
            Some(&ImageReferenceDefinition {
                src: "./ordered.png".to_string(),
                title: None,
            })
        );
    }

    #[test]
    fn ignores_reference_definitions_inside_code_fences_and_html_blocks() {
        let definitions = parse_image_reference_definitions(
            [
                "> ```md",
                "> [code ref]: ./ignored-code.png",
                "> ```",
                "",
                "<div>",
                "[html ref]: ./ignored-html.png",
                "</div>",
                "",
                "> [live ref]: ./real.png",
            ]
            .join("\n")
            .as_str(),
        );

        assert!(!definitions.contains_key("code ref"));
        assert!(!definitions.contains_key("html ref"));
        assert_eq!(
            definitions.get("live ref"),
            Some(&ImageReferenceDefinition {
                src: "./real.png".to_string(),
                title: None,
            })
        );
    }
}
