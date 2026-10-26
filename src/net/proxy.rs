//! 网络代理：偏好里写一条地址，AI 端点、更新检查与远程图片都走它。
//!
//! 两件事分开做：
//!
//! - 我们自己建的 `reqwest` 客户端显式挂 [`reqwest_proxy`]，不看环境变量的时序。
//! - AI provider 用的是第三方库里的 `reqwest::Client::new()`，它只读进程环境，
//!   所以 [`apply_environment`] 把同一份设置落到 `HTTPS_PROXY` / `HTTP_PROXY` /
//!   `ALL_PROXY`（大小写各一份，各家库的读法不同）。
//!
//! 没有设置代理时**不动环境变量**：用户自己 shell 里配的代理照旧生效。

use gpui::App;

// 只有下面两个 `#[cfg(not(test))]` 的函数读它，测试构建里得跟着一起收。
#[cfg(not(test))]
use crate::config::EditorSettings;

/// 环境变量要写的键名：大写是 curl/reqwest 的常规读法，小写是部分工具的要求。
const ENVIRONMENT_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
];

/// 偏好里配置的代理地址；空白串按「没设置」处理。
///
/// 只在生产构建里存在：唯一的调用方是 [`reqwest_proxy`]，而它在测试进程里被
/// 整段绕开（见 net/mod.rs 的 `#[cfg(not(test))]`），留着就是测试构建的死代码。
#[cfg(not(test))]
pub(crate) fn configured_url(cx: &App) -> Option<String> {
    let url = EditorSettings::proxy_url(cx);
    let url = url.trim().to_string();
    (!url.is_empty()).then_some(url)
}

/// 给本应用自建的 `reqwest` 客户端用的代理；地址写错时按直连处理并留下可见日志，
/// 不让一次笔误把所有网络功能一起停掉。
#[cfg(not(test))]
pub(crate) fn reqwest_proxy(cx: &App) -> Option<reqwest::Proxy> {
    let url = configured_url(cx)?;
    match reqwest::Proxy::all(&url) {
        Ok(proxy) => Some(proxy),
        Err(error) => {
            eprintln!("代理地址无法解析，已按直连处理：{error}");
            None
        }
    }
}

/// 没有 `cx` 的兜底路径：更新检查跑在后台线程，直接读盘上的配置。
pub(crate) fn reqwest_proxy_from_disk() -> Option<reqwest::Proxy> {
    let url = crate::config::read_app_preferences()
        .ok()
        .map(|preferences| preferences.proxy_url)
        .unwrap_or_default();
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }
    match reqwest::Proxy::all(&url) {
        Ok(proxy) => Some(proxy),
        Err(error) => {
            eprintln!("代理地址无法解析，已按直连处理：{error}");
            None
        }
    }
}

/// 把当前设置落到进程环境，供第三方 HTTP 客户端读取。
///
/// 只在启动与保存偏好这两处调用：改的是整个进程的环境，此时没有并发读它的线程。
///
/// 测试里不执行：`cargo test` 的全部用例跑在同一个进程里，改环境变量会把并发跑的
/// 其它用例的 HTTP 请求一起导到那个代理上——本地全绿、CI 上整批测试卡到超时。
pub(crate) fn apply_environment(cx: &App) {
    #[cfg(test)]
    {
        let _ = cx;
        return;
    }
    #[cfg(not(test))]
    {
        apply_environment_now(cx)
    }
}

#[cfg(not(test))]
fn apply_environment_now(cx: &App) {
    apply_proxy_environment(configured_url(cx).as_deref());
}

/// 每个代理变量要落的动作：设置成新地址，或从进程环境里移除。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProxyEnvAction {
    Set(String),
    Remove,
}

/// 给定配置（`None` 含空白 = 未配置）算出每个代理变量的目标动作。
/// 纯函数单独拆出来：环境变量是进程全局的，测试不能真改（会把并发用例的
/// HTTP 请求导到代理上），只能钉这份计划——**清空必须对每个键给 Remove**，
/// 否则用户删掉代理后第三方客户端仍沿用进程里残留的旧代理直到重启。
pub(crate) fn proxy_environment_plan(url: Option<&str>) -> Vec<(&'static str, ProxyEnvAction)> {
    match url {
        Some(url) => ENVIRONMENT_KEYS
            .iter()
            .map(|key| (*key, ProxyEnvAction::Set(url.to_string())))
            .collect(),
        None => ENVIRONMENT_KEYS
            .iter()
            .map(|key| (*key, ProxyEnvAction::Remove))
            .collect(),
    }
}

/// 按计划落进程环境。
///
/// # Safety
/// 只在启动早期与保存偏好之后调用（此时没有并发读环境变量的线程），
/// 改的是本进程自己的环境变量。
fn apply_plan(plan: Vec<(&'static str, ProxyEnvAction)>) {
    for (key, action) in plan {
        match action {
            ProxyEnvAction::Set(url) => unsafe { std::env::set_var(key, url) },
            ProxyEnvAction::Remove => unsafe { std::env::remove_var(key) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ENVIRONMENT_KEYS, reqwest_proxy_from_disk};

    /// 盘上写了代理地址，后台线程那条路（更新检查）也要拿得到。
    #[test]
    fn proxy_from_disk_reads_the_configured_address() {
        let root = std::env::temp_dir().join(format!("velora-proxy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let _config = crate::config::override_test_config_root(&root);
        crate::config::update_app_preferences(|preferences| {
            preferences.proxy_url = "http://127.0.0.1:7890".into();
        })
        .expect("写入代理设置");
        assert!(reqwest_proxy_from_disk().is_some(), "有代理时该给出代理");

        crate::config::update_app_preferences(|preferences| {
            preferences.proxy_url = "   ".into();
        })
        .expect("清空代理设置");
        assert!(reqwest_proxy_from_disk().is_none(), "空白地址按未设置处理");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 大小写两份都要有：少一份就有库读不到。
    #[test]
    fn both_letter_cases_are_covered_for_every_proxy_variable() {
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
            assert!(ENVIRONMENT_KEYS.contains(&name));
            let lower = name.to_lowercase();
            assert!(ENVIRONMENT_KEYS.contains(&lower.as_str()));
        }
    }
}
mod plan_tests {
    use super::{ProxyEnvAction, ENVIRONMENT_KEYS, proxy_environment_plan};

    /// 清空代理必须对每个键给 Remove：apply_environment_now 曾在清空分支
    /// 直接 return，set_var 写进去的六个键无人回收，旧代理残留至重启——
    /// AI 请求要么全部失败（旧代理已关）要么继续从已撤销的出口发出。
    #[test]
    fn clearing_the_proxy_plans_removal_for_every_variable() {
        let plan = proxy_environment_plan(Some("http://127.0.0.1:7890"));
        assert_eq!(plan.len(), ENVIRONMENT_KEYS.len());
        assert!(plan
            .iter()
            .all(|(key, action)| ENVIRONMENT_KEYS.contains(key)
                && matches!(action, ProxyEnvAction::Set(url) if url == "http://127.0.0.1:7890")));

        let plan = proxy_environment_plan(None);
        assert_eq!(plan.len(), ENVIRONMENT_KEYS.len());
        assert!(
            plan.iter()
                .all(|(key, action)| ENVIRONMENT_KEYS.contains(key)
                    && matches!(action, ProxyEnvAction::Remove)),
            "清空分支必须 remove 每个代理变量：{plan:?}"
        );
    }
}
