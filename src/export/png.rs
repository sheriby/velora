//! PNG long-image generation through a local Chromium-compatible browser.
//!
//! 与 PDF 导出同源：浏览器 HTML 导出是视觉基准。这里把 HTML 写入临时文件，
//! 用无头 Chromium 打开，量取文档的完整内容尺寸后按该尺寸整页截图，得到
//! 一张包含全文的长图（roadmap F5）。

use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::{
    ClearDeviceMetricsOverrideParams, SetDeviceMetricsOverrideParams,
};
use chromiumoxide::cdp::browser_protocol::page::{CaptureScreenshotFormat, Viewport};
use chromiumoxide::page::{Page, ScreenshotParams};
use futures::StreamExt;

use crate::export::html::render_html_with_base_dir;
use crate::export::{TempHtmlFile, file_url_from_path, unique_temp_path};
use crate::theme::Theme;

/// 长图视口宽度（CSS 像素）：正文容器上限 920px，两侧各留 40px。
pub(crate) const PNG_VIEWPORT_WIDTH: u32 = 1000;
const PNG_VIEWPORT_HEIGHT: u32 = 1600;
/// 设备像素比：2 倍密度让长图放大后文字仍清晰（roadmap F5「清晰度可接受」）。
const PNG_DEVICE_SCALE_FACTOR: f64 = 2.0;
const PNG_TIMEOUT: Duration = Duration::from_secs(45);

/// Renders the whole document as single long PNG bytes.
pub(crate) fn render_png(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    let html = render_html_with_base_dir(markdown, theme, title, base_path);
    render_png_from_html(&html)
}

/// 从已渲染的浏览器 HTML 生成长图；供导出与测试共用。
pub(crate) fn render_png_from_html(html: &str) -> anyhow::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("velotype-png-export")
        .build()
        .context("failed to create PNG export runtime")?;

    runtime.block_on(async move {
        tokio::time::timeout(PNG_TIMEOUT, render_png_html_async(html))
            .await
            .map_err(|_| anyhow!("PNG export timed out while waiting for Chromium"))?
    })
}

async fn render_png_html_async(html: &str) -> anyhow::Result<Vec<u8>> {
    let temp = TempHtmlFile::create(html)?;
    render_png_from_html_file_async(temp.path()).await
}

async fn render_png_from_html_file_async(html_path: &Path) -> anyhow::Result<Vec<u8>> {
    let user_data_dir = unique_temp_path("velotype-chromium-profile");
    fs::create_dir_all(&user_data_dir)
        .with_context(|| format!("failed to create '{}'", user_data_dir.display()))?;

    let config = BrowserConfig::builder()
        .new_headless_mode()
        .window_size(PNG_VIEWPORT_WIDTH, PNG_VIEWPORT_HEIGHT)
        .user_data_dir(user_data_dir.clone())
        .build()
        .map_err(|err| anyhow!("failed to build Chromium browser config: {err}"))?;
    let (mut browser, mut handler) = Browser::launch(config).await.map_err(|err| {
        anyhow!(
            "failed to launch Chromium for PNG export: {err}. Install Chrome, Chromium, or Edge, or set the CHROME environment variable to the browser executable path"
        )
    })?;

    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let result = async {
        let file_url = file_url_from_path(html_path)?;
        let page = browser
            .new_page(file_url.as_str())
            .await
            .context("failed to open export HTML in Chromium")?;
        page.wait_for_navigation()
            .await
            .context("Chromium did not finish loading export HTML")?;

        capture_long_image(&page).await
    }
    .await;

    let _ = browser.close().await;
    handler_task.abort();
    let _ = fs::remove_dir_all(&user_data_dir);

    result
}

/// 量取整页内容尺寸后按该尺寸截图，得到单张长图。
async fn capture_long_image(page: &Page) -> anyhow::Result<Vec<u8>> {
    // 新版无头模式不把 `window_size` 当作页面视口（实测仍是默认 800px 宽），
    // 因此这里用 CDP 固定视口宽度，保证长图版式与正文容器宽度稳定。
    set_viewport(page, PNG_VIEWPORT_HEIGHT as i64).await?;
    let (_, css_height) = content_size(page).await?;

    // 视口高度扩到整页后滚动条消失、宽度回升，需要按重排后的尺寸再量一次。
    let (_, device_height) = device_metrics_size(PNG_VIEWPORT_WIDTH as f64, css_height);
    set_viewport(page, device_height).await?;
    let (css_width, css_height) = content_size(page).await?;

    let image = page
        .screenshot(long_image_params(css_width, css_height))
        .await
        .context("Chromium failed to capture the export page as PNG");
    let _ = page.execute(ClearDeviceMetricsOverrideParams {}).await;

    image
}

async fn set_viewport(page: &Page, css_height: i64) -> anyhow::Result<()> {
    page.execute(SetDeviceMetricsOverrideParams::new(
        PNG_VIEWPORT_WIDTH as i64,
        css_height,
        PNG_DEVICE_SCALE_FACTOR,
        false,
    ))
    .await
    .context("Chromium failed to resize the viewport for PNG export")?;
    Ok(())
}

/// 量取正文的绘制尺寸（CSS 像素）：视口可用宽度 + 文档真实内容高度。
///
/// 不用 `LayoutMetrics.css_content_size`：它给的是「内容与视口的较大者」，
/// 短文档会量到视口高度，长图底部因此多出空白；这里直接问 DOM 要内容高度。
async fn content_size(page: &Page) -> anyhow::Result<(f64, f64)> {
    let measurement = page
        .evaluate(
            "[document.documentElement.clientWidth, document.body.getBoundingClientRect().height]",
        )
        .await
        .context("Chromium failed to measure the export page")?;
    let values = measurement
        .value()
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() == 2)
        .context("Chromium returned no usable page size for PNG export")?;
    let width = values[0].as_f64().unwrap_or(0.0);
    let height = values[1].as_f64().unwrap_or(0.0);
    let (width, height) = (width.max(1.0), height.max(1.0));

    Ok((width, height))
}

/// 量到的 CSS 尺寸向上取整成设备像素尺寸，避免末行像素被裁掉。
fn device_metrics_size(css_width: f64, css_height: f64) -> (i64, i64) {
    let width = css_width.max(1.0).ceil() as i64;
    let height = css_height.max(1.0).ceil() as i64;
    (width, height)
}

/// 全页截图参数：PNG 格式、按内容尺寸裁剪，并允许截图范围超出视口。
fn long_image_params(css_width: f64, css_height: f64) -> ScreenshotParams {
    ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .capture_beyond_viewport(true)
        .clip(Viewport {
            x: 0.,
            y: 0.,
            width: css_width,
            height: css_height,
            scale: 1.,
        })
        .build()
}

#[cfg(test)]
mod tests {
    use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;

    use super::{
        PNG_DEVICE_SCALE_FACTOR, PNG_VIEWPORT_HEIGHT, PNG_VIEWPORT_WIDTH, device_metrics_size,
        long_image_params, render_png, render_png_from_html,
    };
    use crate::export::html::render_html_with_base_dir;
    use crate::theme::Theme;

    #[test]
    fn long_image_params_capture_full_page_as_png() {
        let params = long_image_params(1000.0, 2400.5);

        assert_eq!(params.cdp_params.format, Some(CaptureScreenshotFormat::Png));
        assert_eq!(params.cdp_params.capture_beyond_viewport, Some(true));
        let clip = params.cdp_params.clip.expect("clip");
        assert_eq!(clip.x, 0.);
        assert_eq!(clip.y, 0.);
        assert_eq!(clip.width, 1000.0);
        assert_eq!(clip.height, 2400.5);
        assert_eq!(clip.scale, 1.);
    }

    #[test]
    fn device_metrics_size_rounds_fractional_content_up() {
        assert_eq!(device_metrics_size(1000.0, 2400.2), (1000, 2401));
        assert_eq!(device_metrics_size(999.5, 3000.0), (1000, 3000));
        assert_eq!(device_metrics_size(0.0, 0.0), (1, 1));
    }

    /// 长图走浏览器版式（920px 正文容器 + 主题背景），不带打印分页规则。
    #[test]
    fn long_image_html_uses_browser_layout() {
        let html =
            render_html_with_base_dir("# Title\n\nBody", &Theme::default_theme(), "Doc", None);

        assert!(html.contains("width: min(100% - 48px, 920px)"));
        assert!(!html.contains("@page"));
    }

    #[test]
    fn render_png_reports_actionable_error_without_chromium() {
        match render_png("# Title\n\nBody", &Theme::default_theme(), "Doc", None) {
            Ok(png) => assert!(is_png(&png)),
            Err(err) => assert_actionable_chromium_error(&err.to_string()),
        }
    }

    #[test]
    fn render_png_from_browser_html_uses_chromium_screenshot_pipeline() {
        let html =
            render_html_with_base_dir("# Title\n\nBody", &Theme::default_theme(), "Doc", None);
        match render_png_from_html(&html) {
            Ok(png) => assert!(is_png(&png)),
            Err(err) => assert_actionable_chromium_error(&err.to_string()),
        }
    }

    /// roadmap F5 验收：长图覆盖首屏之外的全部内容，且高度随文档增长。
    #[test]
    fn long_image_height_covers_content_beyond_the_viewport() {
        let short = long_document(200);
        let tall = long_document(400);
        let (short_png, tall_png) = match (
            render_png(&short, &Theme::default_theme(), "short", None),
            render_png(&tall, &Theme::default_theme(), "tall", None),
        ) {
            (Ok(short_png), Ok(tall_png)) => (short_png, tall_png),
            // Chrome 缺失时走可行动错误路径，由上面两个用例覆盖。
            _ => return,
        };

        let (width, short_height) = png_size(&short_png);
        let (_, tall_height) = png_size(&tall_png);

        assert_eq!(width, PNG_VIEWPORT_WIDTH * PNG_DEVICE_SCALE_FACTOR as u32);
        // 只截首屏视口不可能达到这个高度。
        assert!(
            tall_height > PNG_VIEWPORT_HEIGHT * 4,
            "长图高度 {tall_height} 未超出首屏"
        );
        // 段落数翻倍后长图接近翻倍，末段没有被裁掉。
        assert!(
            tall_height * 10 > short_height * 18,
            "长图高度未随文档增长：{short_height} → {tall_height}"
        );
    }

    fn long_document(paragraphs: usize) -> String {
        let mut markdown = String::from("# 长文档\n\n");
        for index in 1..=paragraphs {
            markdown.push_str(&format!("第 {index} 段：长图验收样本。\n\n"));
        }
        markdown
    }

    fn png_size(png: &[u8]) -> (u32, u32) {
        assert!(is_png(png), "expected PNG bytes");
        let width = u32::from_be_bytes(png[16..20].try_into().expect("png width"));
        let height = u32::from_be_bytes(png[20..24].try_into().expect("png height"));
        (width, height)
    }

    fn is_png(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
    }

    fn assert_actionable_chromium_error(message: &str) {
        assert!(
            message.contains("Chromium")
                || message.contains("Chrome")
                || message.contains("CHROME"),
            "unexpected PNG export error: {message}"
        );
    }
}
