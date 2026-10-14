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

pub(crate) fn prepare_install(
    package: &UpdatePackage,
    package_path: &Path,
    launch_args: Vec<String>,
) -> anyhow::Result<InstallPlan> {
    ensure!(package_path.is_file(), "更新安装包不存在");
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
    let directory = plan.package_path.parent().context("安装包目录不存在")?;
    ensure!(plan.package_path.is_file(), "更新安装包不存在");
    let mut command = match plan.platform {
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
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动更新安装程序")?;
    Ok(())
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
# 其他窗口各自是别的进程（命令行启动、`open -n`、或另一份副本）。它们还在跑旧版本时：
# `installer` 往 /Applications 里覆盖这个正在用的 bundle，而 `open` 只会唤醒那个还在跑
# 的旧实例、`--args` 被丢掉——用户看到的还是旧版本在提示更新，与 Windows 那次同源。
/usr/bin/killall -TERM velora 2>/dev/null
# 与 Windows 同一条口径：只请它们自己关（TERM 走正常终止流程，窗口有机会弹「未保存」），
# 等满这个上限还不退就放弃这趟更新——0.5 秒后强杀会把用户正在编辑的内容一起带走。
close_timeout_seconds=30
deadline=$((`/bin/date +%s` + close_timeout_seconds))
while /usr/bin/killall -0 velora 2>/dev/null; do
  if [ "`/bin/date +%s`" -ge "$deadline" ]; then
    /bin/echo '还有其他 Velora 窗口未关闭（请在那些窗口里保存或放弃改动，再更新一次）' >"$error_file"
    "$previous_executable" "$@" >/dev/null 2>&1 &
    exit 0
  fi
  /bin/sleep 0.2
done
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
# 等其他实例自己退的上限：其他窗口里可能正弹着「未保存」提示，用户在那边保存需要时间。
# 到点还没退就放弃这趟更新（写失败原因 + 把旧版本开回来），不硬杀。
$closeTimeoutSeconds = 30
$plan = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8 | ConvertFrom-Json
try {
    try {
        $parent = [System.Diagnostics.Process]::GetProcessById([int]$plan.process_id)
        $parent.WaitForExit()
    } catch [System.ArgumentException] { }
    # 还在跑的实例（多窗口各自成进程）：先请它们自己关。不带 /F 的 taskkill 只发
    # WM_CLOSE，那个窗口会弹出自己的「未保存」提示，用户能在那里真的保存。它们不退就
    # 别装：硬杀会把用户正在编辑的内容一起带走（恢复快照只兜住 debounce 窗口内的输入）。
    & "$env:SystemRoot\System32\taskkill.exe" /IM velora.exe /T 2>$null | Out-Null
    $deadline = (Get-Date).AddSeconds($closeTimeoutSeconds)
    $straggler = { Get-Process -Name velora -ErrorAction SilentlyContinue |
        Where-Object { $_.Id -ne [int]$plan.process_id } }
    while ((Get-Date) -lt $deadline -and (& $straggler)) {
        Start-Sleep -Milliseconds 200
    }
    # 排除发起安装的那个进程号（它刚退出，列表里可能还挂着）：它自己不算「没关」。
    if (& $straggler) {
        throw '还有其他 Velora 窗口未关闭（请在那些窗口里保存或放弃改动，再更新一次）'
    }
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
    /// 更新时别的实例各自成进程（资源管理器再开一个文件就是一个进程，应用内「新建
    /// 窗口」是同一个进程的多个窗口），它们在用着 `velora.exe`，不关掉就覆盖不了。
    /// 但关法要分两步：先请它们自己退（不带 `/F` 的 `taskkill` 只发 WM_CLOSE，那个窗口
    /// 会弹出自己的「未保存」提示，用户能在那里真保存），不退就放弃这趟更新，不硬杀。
    #[test]
    fn windows_update_asks_other_instances_to_close_instead_of_killing_them() {
        let ask = WINDOWS_HELPER
            .lines()
            .find(|line| line.contains("taskkill.exe"))
            .expect("请退那一行");
        assert!(ask.contains("/IM velora.exe /T"), "{ask}");
        assert!(
            !ask.contains("/F"),
            "硬杀会把用户正在编辑的内容一起带走：{ask}"
        );
        assert!(WINDOWS_HELPER.contains("还有其他 Velora 窗口未关闭"));
        assert!(
            WINDOWS_HELPER.contains("Get-Process -Name velora"),
            "请退之后要等到真的退干净：{WINDOWS_HELPER}"
        );
        // 顺序：请退 → 等待 → 还有就放弃 → 才轮到装。
        let ask_at = WINDOWS_HELPER.find("taskkill.exe").expect("请退");
        let install_at = WINDOWS_HELPER.find("$plan.package_path").expect("安装");
        assert!(ask_at < install_at, "先请退再装：{WINDOWS_HELPER}");
    }

    /// 安装包这层只做兜底：应用层（`WINDOWS_HELPER`）已经请过一遍并确认没有实例了，
    /// 这里留着 `/F` 是防「确认之后又有人双击图标」这种竞态，那时窗口里还没有内容。
    #[test]
    fn windows_installer_clears_stragglers_before_overwriting_the_executable() {
        let script = include_str!("../../../scripts/package-windows.nsi");
        assert!(
            script.contains("taskkill.exe\" /IM velora.exe /F /T"),
            "覆盖前要兜住装完又冒出来的实例"
        );
    }

    /// 卸载同样要删 `velora.exe`，但卸载器没有应用层帮它先请一遍，所以它自己请：留着
    /// 没退就不继续删——硬删会把用户正在编辑的内容一起带走。
    #[test]
    fn windows_uninstaller_stops_when_instances_refuse_to_close() {
        let script = include_str!("../../../scripts/package-windows.nsi");
        let uninstall = script
            .split("Section \"Uninstall\"")
            .nth(1)
            .expect("卸载段");
        let ask = uninstall
            .lines()
            .find(|line| line.contains("taskkill.exe"))
            .expect("卸载里的请退那一行");
        assert!(ask.contains("/IM velora.exe /T"), "{ask}");
        assert!(!ask.contains("/F"), "卸载也不硬杀：{ask}");
        assert!(uninstall.contains("MessageBox") && uninstall.contains("Quit"));
        // 判据是「删不掉就说明还锁着」，不能只看进程列表。
        assert!(uninstall.contains("IfFileExists \"$INSTDIR\\velora.exe\""));
    }

    /// macOS 侧收掉其他实例的理由与 Windows 不同（`installer` 覆盖的是正在使用的 bundle，
    /// 而 `open` 只会唤醒还在跑的旧实例、`--args` 被丢掉），但**关法必须同一条口径**：
    /// 先请它们自己退，到点还不退就放弃这趟更新。`killall -KILL` 会把别的窗口里未保存的
    /// 正文一起带走——GPUI 不在 SIGTERM 里跑窗口关闭流程，那句「还有未保存的修改」根本
    /// 弹不出来。
    #[test]
    fn macos_helper_asks_other_instances_to_close_instead_of_killing_them() {
        assert!(MACOS_HELPER.contains("/usr/bin/killall -TERM velora"));
        assert!(
            !MACOS_HELPER.contains("-KILL"),
            "强杀会把其他窗口里未保存的编辑一起带走：{MACOS_HELPER}"
        );
        assert!(
            MACOS_HELPER.contains("还有其他 Velora 窗口未关闭"),
            "不退就该放弃这趟更新：{MACOS_HELPER}"
        );
        let ask = MACOS_HELPER.find("-TERM velora").expect("先请退");
        let poll = MACOS_HELPER
            .find("killall -0 velora")
            .expect("再等它退干净");
        let install = MACOS_HELPER.find("installer -pkg").expect("最后才安装");
        assert!(
            ask < poll && poll < install,
            "先请退、等退干净、再安装：{MACOS_HELPER}"
        );
        assert!(
            MACOS_HELPER.contains("$previous_executable"),
            "放弃这趟要把旧版本开回来"
        );
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
