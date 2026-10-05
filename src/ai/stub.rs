//! 内置演示后端(stub):无网络、确定性地按动作回放剧本。
//!
//! 两个用途:一,测试——整条 AI 链路(面板 → 传输抽象 → 应用/撤销)可以
//! 在没有外部服务的情况下端到端跑通,行为可快照;二,试用——用户装好
//! 应用就能体验完整流程,再换成真实端点。剧本按字符分片输出、片间稍作
//! 停顿并检查取消,与真实流式的时序形态一致。

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::prompts::StubScenario;
use super::AiRequestError;

/// 分片之间的停顿:够看出流式效果,又不拖慢测试。
const CHUNK_PAUSE: Duration = Duration::from_millis(20);
/// 每片输出的字符数。
const CHUNK_CHARS: usize = 6;

/// 按场景回放剧本全文。
pub(crate) fn script(scenario: &StubScenario) -> String {
    match scenario {
        StubScenario::Polish => {
            "这段文字经过润色之后,表达更加流畅自然:句子主干清晰,修饰成分各归其位,而原文的意思与语气都原样保留。".to_string()
        }
        StubScenario::FixGrammar => {
            "这里演示语法纠错:笔误已经改正,标点归于规范,用词与句序保持作者的原样。".to_string()
        }
        StubScenario::Translate(target) => format!(
            "Translation demo (→ {target}):\n\nThis paragraph stands in for a real \
translation. Code blocks, links and identifiers stay untouched."
        ),
        StubScenario::Summarize => {
            "- 要点一:stub 演示会按动作回放对应的剧本。\n- 要点二:输出按流式分片,可随时停止。\n- 要点三:换成真实端点后即可用于生产。".to_string()
        }
        StubScenario::Continue => {
            "接续前文,演示续写会顺着原有的语气与思路自然展开,只返回新增的部分,不重复既有内容。".to_string()
        }
        StubScenario::Rewrite(tone) => {
            format!("改写演示(语气:{tone}):同样的意思换了一种说法,句式重新组织,信息完整保留。")
        }
        StubScenario::Custom => {
            "自定义指令演示:stub 后端按固定剧本回放。接好真实端点后,这里就是模型按你的指令给出的结果。".to_string()
        }
    }
}

/// 把剧本切成字符边界安全的分片。
fn chunk_positions(script: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    let mut count = 0;
    for (index, ch) in script.char_indices() {
        if count == CHUNK_CHARS {
            offsets.push(index);
            count = 0;
        }
        count += ch.len_utf16();
    }
    offsets.push(script.len());
    offsets
}

/// 回放剧本:逐片回调,片间停顿并检查取消。
pub(crate) fn stream(
    scenario: &StubScenario,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<String, AiRequestError> {
    let script = script(scenario);
    let offsets = chunk_positions(&script);
    for window in offsets.windows(2) {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiRequestError::Cancelled);
        }
        let chunk = &script[window[0]..window[1]];
        if !chunk.is_empty() {
            on_delta(chunk);
        }
        thread::sleep(CHUNK_PAUSE);
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    fn collect(scenario: &StubScenario) -> (String, Vec<String>) {
        let mut deltas = Vec::new();
        let full = stream(scenario, &mut |delta| deltas.push(delta.to_string()), &AtomicBool::new(false))
            .expect("stub replay succeeds");
        (full, deltas)
    }

    #[test]
    fn every_scenario_replays_a_nonempty_script() {
        let scenarios = [
            StubScenario::Polish,
            StubScenario::FixGrammar,
            StubScenario::Translate("English"),
            StubScenario::Summarize,
            StubScenario::Continue,
            StubScenario::Rewrite("professional"),
            StubScenario::Custom,
        ];
        for scenario in scenarios {
            let (full, deltas) = collect(&scenario);
            assert!(!full.trim().is_empty(), "{scenario:?} 剧本不应为空");
            assert!(deltas.len() > 1, "{scenario:?} 应分片输出,体现流式");
            // 分片拼回应与全文一致(不丢字、不重复)。
            assert_eq!(deltas.concat(), full);
        }
    }

    #[test]
    fn translate_script_names_the_target_language() {
        let (full, _) = collect(&StubScenario::Translate("Deutsch"));
        assert!(full.contains("Deutsch"));
    }

    #[test]
    fn summarize_script_is_a_bulleted_list() {
        let (full, _) = collect(&StubScenario::Summarize);
        assert!(full.starts_with("- "));
    }

    #[test]
    fn chunks_are_char_boundary_safe() {
        // 中文剧本 3 字节/字符:分片若劈进字符中间,collect 的拼接一致性
        // 断言就会失败;这里再显式验证每片都是合法 UTF-8。
        let scenario = StubScenario::Polish;
        let script = script(&scenario);
        for window in chunk_positions(&script).windows(2) {
            let chunk = &script[window[0]..window[1]];
            assert!(std::str::from_utf8(chunk.as_bytes()).is_ok());
        }
    }

    #[test]
    fn cancel_stops_the_replay() {
        let scenario = StubScenario::Summarize;
        let cancel = AtomicBool::new(true);
        let result = stream(&scenario, &mut |_| {}, &cancel);
        assert_eq!(result, Err(AiRequestError::Cancelled));
    }
}
