mod tests {
    use super::super::{
        ImageReferenceDefinition, ImageResolvedSource, ImageSyntax, ImageTarget,
        TableCellInlineImageSegment, normalize_reference_label, parse_image_reference_definitions,
        parse_standalone_image, parse_table_cell_inline_images, resolve_image_source,
    };
    use std::path::Path;

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
