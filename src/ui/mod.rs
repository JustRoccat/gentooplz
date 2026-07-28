mod widgets;

use crate::model::SystemCompileState;
use crate::View;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::TableState;
use ratatui::Frame;
use std::collections::VecDeque;

pub fn draw(
    frame: &mut Frame,
    state: &SystemCompileState,
    selected: usize,
    cpu_history: &VecDeque<f32>,
    view: View,
    sidebar_table_state: &mut TableState,
) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(frame.area());

    widgets::render_header(frame, outer[0], view);
    widgets::render_rule(frame, outer[1]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(28)])
        .split(outer[2]);

    widgets::render_sidebar(frame, body[0], state, selected, sidebar_table_state);

    match view {
        View::Building => {
            widgets::render_view_building(frame, body[1], state, selected, cpu_history)
        }
        View::Log => widgets::render_view_log(frame, body[1], state, selected),
        View::System => widgets::render_view_system(frame, body[1], state),
    }

    widgets::render_footer(frame, outer[3], state, selected, cpu_history, view);
}
