// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Verify that memory settings reserve enough space for wrapped choices.

use super::*;

#[test]
fn reset_confirmation_keeps_both_choices_at_wrap_boundary() {
    let (app_tx, _app_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut view = MemoriesSettingsView::new(
        /*use_memories*/ true,
        /*generate_memories*/ true,
        AppEventSender::new(app_tx),
        crate::keymap::RuntimeKeymap::defaults().list,
    );
    view.open_reset_confirmation();

    let mut snapshots = Vec::new();
    for width in [68, 69, 70, 71, 72] {
        let area = Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            width,
            view.desired_height(width),
        );
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        let text = buffer
            .content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("Go back"),
            "missing cancel choice at width {width}"
        );
        snapshots.push(format!("width={width}\n{text}"));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn project_reset_never_dispatches_global_reset() {
    let (app_tx, mut app_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut view = MemoriesSettingsView::new(
        true,
        true,
        AppEventSender::new(app_tx),
        crate::keymap::RuntimeKeymap::defaults().list,
    )
    .with_scope("project");
    view.state.selected_idx = view.items.len().checked_sub(1);
    view.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert!(
        matches!(app_rx.try_recv(), Ok(AppEvent::MemoryCommand { args, .. }) if args == "reset-project-unavailable")
    );
    assert!(app_rx.try_recv().is_err());
    assert!(view.reset_confirmation.is_none());
}
