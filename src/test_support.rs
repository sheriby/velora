use std::sync::{Mutex, MutexGuard};

pub(crate) fn exclusive_process_probe() -> MutexGuard<'static, ()> {
    // 单实例检测读取全局进程表，不能与另一个回归启动的同程序子进程重叠。
    static PROCESS_PROBE: Mutex<()> = Mutex::new(());
    PROCESS_PROBE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// mock server 都在 127.0.0.1，loopback 必须绕过系统代理：reqwest 建客户端时会读
/// 系统代理与 `NO_PROXY`，本地请求被代走只会拿回 502。yoagent 的供应商客户端自己
/// 建、不收注入，所以只能往进程环境里补白名单；测试构建里的 net 客户端直接
/// `.no_proxy()`，不走这个开关。
pub(crate) fn bypass_system_proxy_for_loopback() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        const LOOPBACK: &str = "127.0.0.1,localhost,::1";
        let value = match std::env::var("NO_PROXY") {
            Ok(existing) if !existing.trim().is_empty() => format!("{existing},{LOOPBACK}"),
            _ => LOOPBACK.to_string(),
        };
        // SAFETY: 进程内只写这一次，写入的是固定白名单，不涉及调用方状态。
        unsafe {
            std::env::set_var("NO_PROXY", &value);
            std::env::set_var("no_proxy", &value);
        }
    });
}
