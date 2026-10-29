// Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md.
//! One bounded, event-driven summary row for descendants of the displayed conversation root.
//! No key handlers or backend reads live here. Empty summaries consume no terminal rows.

use crate::i18n::text;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::render::renderable::Renderable;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubagentState {
    Running,
    NeedsApproval,
    NeedsInput,
    Completed,
    Failed,
    Stopped,
    Idle,
    Closed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SubagentStatusSummary {
    counts: [usize; 8],
}

impl SubagentStatusSummary {
    pub(crate) fn add(&mut self, state: SubagentState) {
        self.counts[state as usize] += 1;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.counts.iter().all(|count| *count == 0)
    }

    fn line(&self, width: u16) -> Line<'static> {
        let labels = [
            "running",
            "approval needed",
            "input needed",
            "completed",
            "failed",
            "stopped",
            "idle",
            "closed",
        ];
        let mut spans = vec![format!("{}: ", text("Subagents")).dim()];
        for (count, label) in self.counts.iter().zip(labels) {
            if *count == 0 {
                continue;
            }
            if spans.len() > 1 {
                spans.push(" · ".dim());
            }
            spans.push(format!("{} {count}", text(label)).dim());
        }
        const HINT: &str = " · /subagents";
        if usize::from(width) >= HINT.len() + 2 {
            let mut line = truncate_line_with_ellipsis_if_overflow(
                Line::from(spans),
                usize::from(width) - HINT.len(),
            );
            line.push_span(HINT.dim());
            line
        } else {
            truncate_line_with_ellipsis_if_overflow(Line::from("/subagents".dim()), width as usize)
        }
    }
}

impl Renderable for SubagentStatusSummary {
    fn desired_height(&self, width: u16) -> u16 {
        u16::from(!self.is_empty() && width > 0)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.is_empty() && !area.is_empty() {
            Paragraph::new(self.line(area.width)).render(Rect { height: 1, ..area }, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_status_row_bounds_width_and_keeps_command_hint() {
        let mut summary = SubagentStatusSummary::default();
        for state in [
            SubagentState::Running,
            SubagentState::NeedsApproval,
            SubagentState::NeedsInput,
            SubagentState::Completed,
            SubagentState::Failed,
            SubagentState::Stopped,
            SubagentState::Idle,
            SubagentState::Closed,
        ] {
            summary.add(state);
        }
        crate::i18n::with_locale(crate::i18n::Locale::English, || {
            assert_eq!(
                summary.line(240).to_string(),
                "Subagents: running 1 · approval needed 1 · input needed 1 · completed 1 · failed 1 · stopped 1 · idle 1 · closed 1 · /subagents"
            );
        });
        for language in [crate::i18n::Locale::English, crate::i18n::Locale::Korean] {
            crate::i18n::with_locale(language, || {
                for width in [0, 1, 8, 14, 24, 80, 200] {
                    let line = summary.line(width);
                    assert!(line.width() <= usize::from(width));
                    if width >= 14 {
                        assert!(line.to_string().ends_with("/subagents"));
                    }
                    assert_eq!(summary.desired_height(width), u16::from(width > 0));
                }
            });
        }
    }
}
