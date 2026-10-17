use std::sync::{Mutex, MutexGuard};

pub(crate) fn exclusive_process_probe() -> MutexGuard<'static, ()> {
    // 单实例检测读取全局进程表，不能与另一个回归启动的同程序子进程重叠。
    static PROCESS_PROBE: Mutex<()> = Mutex::new(());
    PROCESS_PROBE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
