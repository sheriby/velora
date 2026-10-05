//! 编辑器测试入口：原单文件用例按主题拆分为子模块，共享脚手架在
//! [`common`]（`init_editor_test_app`、`redraw`、临时路径与性能计数等），
//! 各子模块通过 `use super::common::*` 引入。纯移动拆分，用例本身未改。

mod block_source_spans;
mod block_source_write_back;
mod common;
mod document_context_menu;

mod editing_misc;
mod export_drop;
mod footnotes;
mod format_command;
mod image_runtime;
mod import_perf;
mod inline_format;
mod keyboard_nav;
mod knowledge_history;
mod knowledge_recovery;
mod loading_chunks;
mod long_lines;
mod perf_budgets;
mod paragraph_kind;
mod read_side_sees_the_file;
mod render_snapshot_fold;
mod round_trip_fidelity;
mod save_autosave_ime;
mod scroll_window;
mod source_selection;
mod selection_mouse;
mod table_runtime;
mod tree_quick_open;
mod typing_wrapping;
mod undo_history_deltas;
mod window_menu;
mod workspace_shell;
