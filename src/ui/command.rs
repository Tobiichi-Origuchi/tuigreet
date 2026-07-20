use ratatui::{
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  widgets::{Block, BorderType, Borders, Paragraph},
};

use super::common::style::Themed;
use crate::{
  Greeter,
  ui::{Frame, input, prompt_value, util::*},
};

pub fn draw(greeter: &Greeter, f: &mut Frame, area: Rect) -> Option<(u16, u16)> {
  let theme = &greeter.theme;
  let container_padding = greeter.container_padding();
  let width = greeter.width().min(area.width);
  let content_width = width.saturating_sub(container_padding.saturating_mul(2));
  let container_height = get_height(greeter, content_width);
  let (warning, warning_height) = get_message_height(greeter.input_warning.as_deref(), width);
  let feedback = feedback_layout(area, width, container_height, container_height, warning_height);
  let container = feedback.container;
  let frame = inset(container, container_padding);

  let block = Block::default()
    .title(titleize(&greeter.text.title_command))
    .title_style(theme.of(&[Themed::Title]))
    .style(theme.of(&[Themed::Container]))
    .borders(Borders::ALL)
    .border_type(BorderType::Plain)
    .border_style(theme.of(&[Themed::Border]));

  f.render_widget(block, container);

  let constraints = [
    Constraint::Length(1), // Username
  ];

  let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints(constraints.as_ref())
    .split(frame);
  let cursor = chunks[0];

  let command_label_text = prompt_value(theme, Some(greeter.text.new_command.as_str()));
  let command_label = Paragraph::new(command_label_text).style(theme.of(&[Themed::Prompt]));

  f.render_widget(command_label, chunks[0]);
  let input_area = input_area(cursor, &greeter.text.new_command);
  let cursor = if input_area.width == 0 || input_area.height == 0 {
    None
  } else {
    let view = input::view(&greeter.command_buffer, greeter.command_cursor, input_area.width);
    let command_value = Paragraph::new(view.text).style(theme.of(&[Themed::Input]));
    f.render_widget(command_value, input_area);
    Some((input_area.x.saturating_add(view.cursor_column), input_area.y))
  };

  if let Some(warning) = warning {
    let warning = warning
      .alignment(Alignment::Center)
      .scroll((feedback.message_scroll, 0));
    f.render_widget(warning, feedback.message);
  }

  cursor
}
