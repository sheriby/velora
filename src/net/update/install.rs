//! 安装器在当前进程退出后运行，避免替换被 Windows 占用的可执行文件。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};
use serde::Serialize;

use super::{UpdatePackage, UpdatePlatform, updates_dir};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct InstallPlan {
    pub(crate) package_path: PathBuf,
    pub(crate) platform: UpdatePlatform,
    pub(crate) process_id: u32,
    pub(crate) install_dir: PathBuf,
    pub(crate) launch_path: PathBuf,
    pub(crate) previous_executable: PathBuf,
    pub(crate) launch_arguments: String,
    pub(crate) launch_args: Vec<String>,
    pub(crate) error_path: PathBuf,
}

pub(crate) fn ensure_no_other_instances() -> anyhow::Result<()> {
    // GPUI 的窗口列表只覆盖当前进程，另一次启动的工作区会继续占用安装目标。
    let executable = std::env::current_exe()?;
    let executable_name = executable
        .file_name()
        .context("无法确定 Velora 程序名")?
        .to_string_lossy();
    let current_process = sysinfo::Pid::from_u32(std::process::id());
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        sysinfo::ProcessRefreshKind::new().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
    );
    ensure!(
        system.process(current_process).is_some(),
        "无法检查其他 Velora 实例，已停止更新。请关闭其他 Velora 窗口后重试。"
    );
    let other_instances = system
        .processes()
        .values()
        .filter(|process| {
            process.pid() != current_process
                && (process.exe() == Some(executable.as_path())
                    || process
                        .name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&executable_name))
        })
        .count();
    ensure!(
        other_instances == 0,
        "还有 {other_instances} 个 Velora 工作区程序在运行。请先保存并关闭其他 Velora 窗口，再重试更新。本次更新已停止，当前窗口不会退出。"
    );
    Ok(())
}

pub(crate) fn prepare_install(
    package: &UpdatePackage,
    package_path: &Path,
    launch_args: Vec<String>,
) -> anyhow::Result<InstallPlan> {
    ensure!(package_path.is_file(), "更新安装包不存在");
    ensure_no_other_instances()?;
    prepare_install_plan(package, package_path, launch_args)
}

pub(crate) fn prepare_install_plan(
    package: &UpdatePackage,
    package_path: &Path,
    launch_args: Vec<String>,
) -> anyhow::Result<InstallPlan> {
    let previous_executable = std::env::current_exe()?;
    let install_dir = match package.platform {
        UpdatePlatform::MacOsArm64 | UpdatePlatform::MacOsX64 => PathBuf::from("/Applications"),
        UpdatePlatform::WindowsX64 => {
            let existing = previous_executable
                .parent()
                .filter(|parent| parent.join("Uninstall.exe").is_file());
            match existing {
                Some(parent) => parent.to_path_buf(),
                None => PathBuf::from(
                    std::env::var_os("LOCALAPPDATA").context("无法确定 Windows 安装目录")?,
                )
                .join("Programs")
                .join("Velora"),
            }
        }
    };
    let launch_path = match package.platform {
        UpdatePlatform::WindowsX64 => install_dir.join("velora.exe"),
        _ => install_dir.join("velora.app"),
    };
    let launch_arguments = launch_args
        .iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(InstallPlan {
        package_path: package_path.into(),
        platform: package.platform,
        process_id: std::process::id(),
        install_dir,
        launch_path,
        previous_executable,
        launch_arguments,
        launch_args,
        error_path: updates_dir()?.join("install-error.txt"),
    })
}

pub(crate) fn launch_install_helper(plan: &InstallPlan) -> anyhow::Result<()> {
    install_helper_command(plan)?
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动更新安装程序")?;
    Ok(())
}

fn install_helper_command(plan: &InstallPlan) -> anyhow::Result<Command> {
    // 下载和保存期间可能又打开了其他工作区，退出前必须重新检查占用。
    ensure_no_other_instances()?;
    let directory = plan.package_path.parent().context("安装包目录不存在")?;
    ensure!(plan.package_path.is_file(), "更新安装包不存在");
    let command = match plan.platform {
        UpdatePlatform::MacOsArm64 | UpdatePlatform::MacOsX64 => {
            ensure!(cfg!(target_os = "macos"), "当前系统不能安装 macOS 更新");
            let script = directory.join("install-update.sh");
            fs::write(&script, MACOS_HELPER)?;
            let mut command = Command::new("/bin/sh");
            command
                .arg(script)
                .arg(plan.process_id.to_string())
                .arg(&plan.package_path)
                .arg(&plan.launch_path)
                .arg(&plan.error_path)
                .arg(&plan.previous_executable)
                .args(&plan.launch_args);
            command
        }
        UpdatePlatform::WindowsX64 => {
            ensure!(cfg!(target_os = "windows"), "当前系统不能安装 Windows 更新");
            let script = directory.join("install-update.ps1");
            let config = directory.join("install-plan.json");
            fs::write(&script, WINDOWS_HELPER)?;
            fs::write(&config, serde_json::to_vec(plan)?)?;
            let system_root = std::env::var_os("SystemRoot").context("Windows 系统目录不可用")?;
            let executable = PathBuf::from(system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
            let mut command = Command::new(executable);
            command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(script)
                .arg("-ConfigPath")
                .arg(config);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command
        }
    };
    Ok(command)
}

pub(crate) fn take_install_error() -> anyhow::Result<Option<String>> {
    let path = updates_dir()?.join("install-error.txt");
    match fs::read_to_string(&path) {
        Ok(detail) => {
            fs::remove_file(path)?;
            Ok(Some(detail.trim_start_matches('\u{feff}').trim().into()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn quote_windows_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        quoted.extend(std::iter::repeat_n(
            '\\',
            if character == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            },
        ));
        quoted.push(character);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

const MACOS_HELPER: &str = r#"#!/bin/sh
process_id="$1"
package="$2"
application="$3"
error_file="$4"
previous_executable="$5"
shift 5
while /bin/kill -0 "$process_id" 2>/dev/null; do /bin/sleep 0.2; done
/usr/bin/osascript -e 'on run argv
  do shell script "/usr/sbin/installer -pkg " & quoted form of (item 1 of argv) & " -target /" with administrator privileges
end run' "$package" >"$error_file" 2>&1
status=$?
if [ "$status" -eq 0 ]; then
  /bin/rm -f "$error_file"
  /usr/bin/open "$application" --args "$@"
  /bin/rm -f "$package"
else
  "$previous_executable" "$@" >/dev/null 2>&1 &
fi
"#;

const WINDOWS_HELPER: &str = r#"param([string]$ConfigPath)
$ErrorActionPreference = 'Stop'
$plan = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8 | ConvertFrom-Json
try {
    try {
        $parent = [System.Diagnostics.Process]::GetProcessById([int]$plan.process_id)
        $parent.WaitForExit()
    } catch [System.ArgumentException] { }
    $settings = New-Object System.Diagnostics.ProcessStartInfo
    $settings.FileName = $plan.package_path
    $settings.Arguments = '/S /D=' + $plan.install_dir
    $settings.UseShellExecute = $false
    $installer = [System.Diagnostics.Process]::Start($settings)
    $installer.WaitForExit()
    if ($installer.ExitCode -ne 0) { throw ('Installer exit code: ' + $installer.ExitCode) }
    if (Test-Path -LiteralPath $plan.error_path) { Remove-Item -LiteralPath $plan.error_path }
    $settings.FileName = $plan.launch_path
    $settings.Arguments = $plan.launch_arguments
    [System.Diagnostics.Process]::Start($settings) | Out-Null
    try { Remove-Item -LiteralPath $plan.package_path } catch { Write-Warning $_.Exception.Message }
} catch {
    $_.Exception.Message | Set-Content -LiteralPath $plan.error_path -Encoding UTF8
    $settings = New-Object System.Diagnostics.ProcessStartInfo
    $settings.FileName = $plan.previous_executable
    $settings.Arguments = $plan.launch_arguments
    $settings.UseShellExecute = $false
    [System.Diagnostics.Process]::Start($settings) | Out-Null
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_instance_probe() {
        let Some(directory) = std::env::var_os("VELORA_UPDATE_INSTANCE_PROBE") else {
            return;
        };
        let directory = PathBuf::from(directory);
        fs::write(directory.join("ready"), b"").expect("实例已启动");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !directory.join("stop").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
    fn another_running_instance_blocks_update_before_the_current_workspace_exits() {
        let _process_guard = crate::test_support::exclusive_process_probe();
        const ISOLATED: &str = "VELORA_UPDATE_ISOLATED_TEST";
        if std::env::var_os(ISOLATED).is_none() {
            // 全量回归中的其他子进程共享测试程序名，实例计数需要独立的程序身份。
            let identity = uuid::Uuid::new_v4().simple().to_string();
            let directory = std::env::temp_dir().join(format!("velora-instance-test-{identity}"));
            fs::create_dir_all(&directory).expect("独立测试目录");
            let executable = directory.join(format!("vu-{}{}", &identity[..10], if cfg!(target_os = "windows") { ".exe" } else { "" }));
            fs::copy(std::env::current_exe().expect("测试程序"), &executable).expect("独立程序副本");
            let output = Command::new(&executable)
                .args(["--exact", "net::update::install::tests::another_running_instance_blocks_update_before_the_current_workspace_exits", "--ignored", "--nocapture"])
                .env(ISOLATED, "1")
                .output();
            fs::remove_dir_all(&directory).expect("清理独立测试程序");
            let output = output.expect("启动独立实例回归");
            assert!(output.status.success(), "独立实例回归失败：{}\n{}\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            return;
        }
        struct Probe(std::process::Child);
        impl Drop for Probe {
            fn drop(&mut self) {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    return;
                }
                if let Err(error) = self.0.kill() {
                    eprintln!("结束测试实例：{error}");
                }
                if let Err(error) = self.0.wait() {
                    eprintln!("等待测试实例结束：{error}");
                }
            }
        }
        let directory = std::env::temp_dir()
            .join(format!("velora-update-instance-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("测试目录");
        let _config = crate::config::override_test_config_root(directory.join("config"));
        let package_path = directory.join("package");
        fs::write(&package_path, b"fixture").expect("测试安装包");
        let package = super::super::UpdatePackage {
            name: "package".into(),
            url: String::new(),
            size: 7,
            digest: None,
            platform: UpdatePlatform::current().expect("支持的系统"),
        };
        let plan = prepare_install(&package, &package_path, Vec::new()).expect("单实例允许更新");
        let mut probe = Probe(
            Command::new(std::env::current_exe().expect("测试程序"))
                .args(["--exact", "net::update::install::tests::update_instance_probe"])
                .env("VELORA_UPDATE_INSTANCE_PROBE", &directory)
                .stdout(Stdio::null())
                .spawn()
                .expect("启动另一个工作区进程"),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !directory.join("ready").exists() && std::time::Instant::now() < deadline {
            assert!(
                probe.0.try_wait().expect("实例状态").is_none(),
                "测试实例提前退出"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(directory.join("ready").is_file(), "测试实例未就绪");
        let result = prepare_install(&package, &package_path, Vec::new());
        assert!(
            result.is_err(),
            "还有另一个工作区进程时，不应允许当前工作区退出并安装"
        );
        let detail = result.expect_err("更新应被阻止").to_string();
        assert!(
            detail.contains("Velora") && detail.contains("关闭"),
            "应明确提示关闭其他工作区：{detail}"
        );
        assert!(
            install_helper_command(&plan).is_err(),
            "生成安装计划后又打开的实例，也必须阻止安装辅助程序启动"
        );
        assert!(!directory.join("install-update.sh").exists());
        assert!(!directory.join("install-update.ps1").exists());
        assert!(
            probe.0.try_wait().expect("实例状态").is_none(),
            "不得强杀其他工作区"
        );
        fs::write(directory.join("stop"), b"").expect("请求测试实例退出");
        assert!(probe.0.wait().expect("实例退出").success());
        assert!(
            prepare_install(&package, &package_path, Vec::new()).is_ok(),
            "其他实例退出后应能重新更新"
        );
        fs::remove_dir_all(directory).expect("清理测试目录");
    }

    #[test]
    fn installer_helpers_wait_for_exit_and_use_installer_arguments_as_data() {
        assert!(MACOS_HELPER.contains("while /bin/kill -0"));
        assert!(MACOS_HELPER.contains("quoted form of (item 1 of argv)"));
        assert!(WINDOWS_HELPER.contains("$parent.WaitForExit()"));
        assert!(WINDOWS_HELPER.contains("'/S /D=' + $plan.install_dir"));
        assert!(!WINDOWS_HELPER.contains("Invoke-Expression"));
    }
    #[test]
    fn windows_restart_arguments_preserve_quotes_spaces_and_trailing_slashes() {
        assert_eq!(
            quote_windows_argument("C:\\我的文档\\笔记.md"),
            "\"C:\\我的文档\\笔记.md\""
        );
        assert_eq!(
            quote_windows_argument("C:\\My Notes\\"),
            "\"C:\\My Notes\\\\\""
        );
        assert_eq!(quote_windows_argument("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_installer_helper_parses_without_executing_installation() {
        use std::io::Write;
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-n")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("shell parser");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(MACOS_HELPER.as_bytes())
            .expect("script");
        assert!(child.wait().expect("parser status").success());
        let output = std::env::temp_dir().join(format!(
            "velora-installer-script-{}.scpt",
            uuid::Uuid::new_v4()
        ));
        let status = std::process::Command::new("/usr/bin/osacompile").arg("-o").arg(&output).arg("-e")
            .arg("on run argv\n do shell script \"/usr/sbin/installer -pkg \" & quoted form of (item 1 of argv) & \" -target /\" with administrator privileges\nend run")
            .status().expect("AppleScript compiler");
        assert!(status.success());
        std::fs::remove_file(output).expect("cleanup");
    }
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_installer_helper_parses_without_executing_installation() {
        use std::io::Write;
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile","-NonInteractive","-Command", "$tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseInput([Console]::In.ReadToEnd(), [ref]$tokens, [ref]$errors) | Out-Null; if($errors.Count -gt 0) { Write-Error ($errors | Out-String); exit 1 }"])
            .stdin(std::process::Stdio::piped()).spawn().expect("PowerShell parser");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(WINDOWS_HELPER.as_bytes())
            .expect("script");
        assert!(child.wait().expect("parser status").success());
    }
}
