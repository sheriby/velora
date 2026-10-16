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
