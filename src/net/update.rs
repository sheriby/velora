//! 已发布版本查询和安装包下载。未发布的分支版本不参与升级判断。

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub(crate) const RELEASES_URL: &str = "https://github.com/sheriby/velora/releases";
const RELEASES_API_URL: &str = "https://api.github.com/repos/sheriby/velora/releases?per_page=100";
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

mod install;
pub(crate) use install::{InstallPlan, launch_install_helper, prepare_install, take_install_error};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateSource {
    GitHub,
}
impl fmt::Display for UpdateSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GitHub")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) enum UpdatePlatform {
    MacOsArm64,
    MacOsX64,
    WindowsX64,
}
impl UpdatePlatform {
    pub(crate) fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(if cfg!(target_arch = "aarch64") {
                Self::MacOsArm64
            } else {
                Self::MacOsX64
            })
        } else if cfg!(target_os = "windows") {
            Some(Self::WindowsX64)
        } else {
            None
        }
    }
    pub(crate) fn asset_name(self, version: &Version) -> String {
        let suffix = match self {
            Self::MacOsArm64 => "macos-arm64.pkg",
            Self::MacOsX64 => "macos-x64.pkg",
            Self::WindowsX64 => "windows-x64-setup.exe",
        };
        format!("velora-{version}-{suffix}")
    }
    fn magic(self) -> &'static [u8] {
        match self {
            Self::WindowsX64 => b"MZ",
            _ => b"xar!",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct UpdatePackage {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) size: u64,
    pub(crate) digest: Option<String>,
    pub(crate) platform: UpdatePlatform,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateVersionInfo {
    pub(crate) current_version: String,
    pub(crate) latest_version: String,
    pub(crate) source: UpdateSource,
    pub(crate) release_url: String,
    pub(crate) package: Option<UpdatePackage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateCheckResult {
    UpdateAvailable(UpdateVersionInfo),
    UpToDate(UpdateVersionInfo),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateCheckError {
    Fetch(String),
    ParseVersion(String),
}
impl fmt::Display for UpdateCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(detail) | Self::ParseVersion(detail) => formatter.write_str(detail),
        }
    }
}
impl std::error::Error for UpdateCheckError {}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}
#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    state: String,
    #[serde(default)]
    digest: Option<String>,
}

pub(crate) fn check_latest_version(
    current_version: &str,
) -> Result<UpdateCheckResult, UpdateCheckError> {
    check_latest_version_with_options(current_version, false)
}
pub(crate) fn check_latest_version_with_options(
    current_version: &str,
    include_prereleases: bool,
) -> Result<UpdateCheckResult, UpdateCheckError> {
    let platform = UpdatePlatform::current()
        .ok_or_else(|| UpdateCheckError::Fetch("当前平台没有可用的安装包".into()))?;
    let client = update_client(Duration::from_secs(15))
        .map_err(|error| UpdateCheckError::Fetch(error.to_string()))?;
    let mut body = String::new();
    client
        .get(RELEASES_API_URL)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| UpdateCheckError::Fetch(error.to_string()))?
        .take(8 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|error| UpdateCheckError::Fetch(error.to_string()))?;
    select_release(&body, current_version, include_prereleases, platform)
}

#[cfg(test)]
fn check_latest_version_with<F>(
    current_version: &str,
    mut fetch: F,
) -> Result<UpdateCheckResult, UpdateCheckError>
where
    F: FnMut(UpdateSource) -> Result<String, UpdateCheckError>,
{
    select_release(
        &fetch(UpdateSource::GitHub)?,
        current_version,
        false,
        UpdatePlatform::MacOsArm64,
    )
}

fn select_release(
    body: &str,
    current_version: &str,
    include_prereleases: bool,
    platform: UpdatePlatform,
) -> Result<UpdateCheckResult, UpdateCheckError> {
    let current = Version::parse(current_version)
        .map_err(|error| UpdateCheckError::ParseVersion(error.to_string()))?;
    let releases: Vec<Release> = serde_json::from_str(body)
        .map_err(|error| UpdateCheckError::Fetch(format!("无法读取 GitHub Release：{error}")))?;
    let latest = releases
        .into_iter()
        .filter_map(|release| {
            if release.draft || release.published_at.is_none() {
                return None;
            }
            let version = match Version::parse(
                release
                    .tag_name
                    .strip_prefix('v')
                    .unwrap_or(&release.tag_name),
            ) {
                Ok(version) => version,
                Err(error) => {
                    eprintln!("忽略无效发布标签 {}：{error}", release.tag_name);
                    return None;
                }
            };
            if !include_prereleases && (release.prerelease || !version.pre.is_empty()) {
                return None;
            }
            Some((version, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .ok_or_else(|| UpdateCheckError::Fetch("尚无可用的已发布版本".into()))?;
    let (version, release) = latest;
    let mut info = UpdateVersionInfo {
        current_version: current_version.into(),
        latest_version: version.to_string(),
        source: UpdateSource::GitHub,
        release_url: release.html_url,
        package: None,
    };
    if version <= current {
        return Ok(UpdateCheckResult::UpToDate(info));
    }
    let expected_name = platform.asset_name(&version);
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == expected_name && asset.state == "uploaded")
        .ok_or_else(|| UpdateCheckError::Fetch("新版本暂时没有适用于当前系统的安装包".into()))?;
    let package = UpdatePackage {
        name: asset.name,
        url: asset.browser_download_url,
        size: asset.size,
        digest: asset.digest,
        platform,
    };
    validate_package(&package).map_err(|error| UpdateCheckError::Fetch(error.to_string()))?;
    info.package = Some(package);
    Ok(UpdateCheckResult::UpdateAvailable(info))
}

fn update_client(timeout: Duration) -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("更新请求禁止跳转到非 HTTPS 地址")
            } else if attempt.previous().len() >= 10 {
                attempt.error("更新请求跳转次数过多")
            } else {
                attempt.follow()
            }
        }))
        .default_headers(update_request_headers())
        .build()
        .context("创建更新连接失败")
}
fn update_request_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(concat!("Velora/", env!("CARGO_PKG_VERSION"))),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2026-03-10"),
    );
    headers
}
fn validate_package(package: &UpdatePackage) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(&package.url)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && package.url.starts_with(&format!("{RELEASES_URL}/download/"))
            && url.username().is_empty()
            && url.password().is_none(),
        "安装包地址不属于 Velora 官方发布"
    );
    ensure!(
        package.size > 0 && package.size <= MAX_PACKAGE_BYTES,
        "安装包大小无效"
    );
    ensure!(
        Path::new(&package.name)
            .file_name()
            .is_some_and(|name| name == package.name.as_str()),
        "安装包名称无效"
    );
    if let Some(digest) = &package.digest {
        let checksum = digest
            .strip_prefix("sha256:")
            .context("不支持的安装包校验类型")?;
        ensure!(
            checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "安装包校验值无效"
        );
    }
    Ok(())
}

pub(crate) fn updates_dir() -> anyhow::Result<PathBuf> {
    Ok(crate::config::VeloraConfigDirs::from_system()?
        .app_config_file()
        .with_file_name("updates"))
}
pub(crate) fn download_package(
    package: &UpdatePackage,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> anyhow::Result<PathBuf> {
    validate_package(package)?;
    ensure!(package.digest.is_some(), "发布版本缺少安装包校验信息");
    let directory = updates_dir()?.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&directory)?;
    let target = directory.join(&package.name);
    let partial = target.with_extension("part");
    let result = (|| {
        let response = update_client(Duration::from_secs(600))?
            .get(&package.url)
            .send()?
            .error_for_status()?;
        let mut file = fs::File::create(&partial)?;
        receive_package(response, package, &mut file, cancelled, &mut progress)?;
        file.sync_all()?;
        fs::rename(&partial, &target)?;
        Ok(target)
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_dir_all(&directory) {
            eprintln!("清理更新下载失败：{error}");
        }
    }
    result
}
fn receive_package(
    mut reader: impl Read,
    package: &UpdatePackage,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> anyhow::Result<()> {
    let mut bytes = [0u8; 65536];
    let mut total = 0u64;
    let mut checksum = Sha256::new();
    let mut magic = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            bail!("更新下载已取消");
        }
        let count = reader.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(
            total <= package.size && total <= MAX_PACKAGE_BYTES,
            "安装包超过声明的大小"
        );
        if magic.len() < 4 {
            magic.extend(bytes.iter().take(count.min(4 - magic.len())));
        }
        checksum.update(&bytes[..count]);
        writer.write_all(&bytes[..count])?;
        progress(total);
    }
    ensure!(total == package.size, "安装包下载不完整");
    ensure!(
        magic.starts_with(package.platform.magic()),
        "安装包格式无效"
    );
    let actual = format!("{:x}", checksum.finalize());
    let expected = package
        .digest
        .as_ref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .context("发布版本缺少安装包校验信息")?;
    ensure!(actual.eq_ignore_ascii_case(expected), "安装包校验失败");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(
        version: &str,
        prerelease: bool,
        draft: bool,
        platform: UpdatePlatform,
    ) -> serde_json::Value {
        let version = Version::parse(version).unwrap();
        let name = platform.asset_name(&version);
        serde_json::json!({"tag_name":format!("v{version}"),"html_url":RELEASES_URL,"draft":draft,"prerelease":prerelease,"published_at":"2026-10-10T00:00:00Z","assets":[{"name":name,"browser_download_url":format!("{RELEASES_URL}/download/v{version}/{name}"),"size":8,"state":"uploaded"}]})
    }
    #[test]
    fn checks_a_published_release_instead_of_the_branch_manifest() {
        let body = serde_json::to_string(&vec![release(
            "0.2.5",
            false,
            false,
            UpdatePlatform::MacOsArm64,
        )])
        .unwrap();
        assert!(
            matches!(check_latest_version_with("0.2.4", |_| Ok(body.clone())), Ok(UpdateCheckResult::UpdateAvailable(info)) if info.latest_version == "0.2.5")
        );
    }
    #[test]
    fn stable_channel_skips_beta_and_draft_releases() {
        let body = serde_json::to_string(&vec![
            release("0.2.6-beta.1", true, false, UpdatePlatform::MacOsArm64),
            release("0.2.7", false, true, UpdatePlatform::MacOsArm64),
            release("0.2.5", false, false, UpdatePlatform::MacOsArm64),
        ])
        .unwrap();
        let result = select_release(&body, "0.2.4", false, UpdatePlatform::MacOsArm64).unwrap();
        assert!(
            matches!(result,UpdateCheckResult::UpdateAvailable(info) if info.latest_version == "0.2.5")
        );
        let result = select_release(&body, "0.2.4", true, UpdatePlatform::MacOsArm64).unwrap();
        assert!(
            matches!(result,UpdateCheckResult::UpdateAvailable(info) if info.latest_version == "0.2.6-beta.1")
        );
    }
    #[test]
    fn installer_selection_matches_each_supported_architecture() {
        for platform in [
            UpdatePlatform::MacOsArm64,
            UpdatePlatform::MacOsX64,
            UpdatePlatform::WindowsX64,
        ] {
            let body =
                serde_json::to_string(&vec![release("0.2.5", false, false, platform)]).unwrap();
            let UpdateCheckResult::UpdateAvailable(info) =
                select_release(&body, "0.2.4", false, platform).unwrap()
            else {
                panic!("更新");
            };
            assert_eq!(info.package.unwrap().platform, platform);
        }
    }
    #[test]
    fn current_or_newer_build_does_not_require_an_installer() {
        let body = serde_json::to_string(&vec![release(
            "0.2.4",
            false,
            false,
            UpdatePlatform::MacOsArm64,
        )])
        .unwrap();
        assert!(matches!(
            select_release(&body, "0.2.5", false, UpdatePlatform::WindowsX64),
            Ok(UpdateCheckResult::UpToDate(_))
        ));
    }
    #[test]
    fn verified_download_rejects_corruption_wrong_format_and_truncation() {
        let bytes = b"xar!test";
        let package = UpdatePackage {
            name: "velora.pkg".into(),
            url: format!("{RELEASES_URL}/download/v0.2.5/velora.pkg"),
            size: 8,
            digest: Some(format!("sha256:{:x}", Sha256::digest(bytes))),
            platform: UpdatePlatform::MacOsArm64,
        };
        let cancelled = AtomicBool::new(false);
        assert!(
            receive_package(
                std::io::Cursor::new(bytes),
                &package,
                &mut Vec::new(),
                &cancelled,
                &mut |_| {}
            )
            .is_ok()
        );
        for source in [
            b"xar!bad!".as_slice(),
            b"notapkg!".as_slice(),
            b"xar!".as_slice(),
        ] {
            assert!(
                receive_package(
                    std::io::Cursor::new(source),
                    &package,
                    &mut Vec::new(),
                    &cancelled,
                    &mut |_| {}
                )
                .is_err()
            );
        }
        cancelled.store(true, Ordering::Relaxed);
        assert!(
            receive_package(
                std::io::Cursor::new(bytes),
                &package,
                &mut Vec::new(),
                &cancelled,
                &mut |_| {}
            )
            .is_err()
        );
    }
    #[test]
    fn installer_download_refuses_unofficial_hosts() {
        let mut package = UpdatePackage {
            name: "velora.pkg".into(),
            url: "https://example.com/velora.pkg".into(),
            size: 8,
            digest: None,
            platform: UpdatePlatform::MacOsArm64,
        };
        assert!(validate_package(&package).is_err());
        package.url = "http://github.com/sheriby/velora/releases/download/v0.2.5/velora.pkg".into();
        assert!(validate_package(&package).is_err());
    }
}
