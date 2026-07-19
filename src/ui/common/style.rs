use std::str::FromStr;

use ratatui::style::{Color, Style};

use crate::config::{ThemeColor, ThemeSettings};

#[derive(Clone)]
enum Component {
  Bg,
  Fg,
}

#[derive(Clone, Copy)]
pub enum Themed {
  Container,
  Time,
  Text,
  Border,
  Title,
  Greet,
  Prompt,
  Input,
  Action,
  ActionButton,
}

#[derive(Default)]
pub struct Theme {
  container: Option<(Component, Color)>,
  time: Option<(Component, Color)>,
  text: Option<(Component, Color)>,
  border: Option<(Component, Color)>,
  title: Option<(Component, Color)>,
  greet: Option<(Component, Color)>,
  prompt: Option<(Component, Color)>,
  input: Option<(Component, Color)>,
  action: Option<(Component, Color)>,
  button: Option<(Component, Color)>,
}

impl Theme {
  pub fn from_settings(settings: &ThemeSettings) -> Theme {
    use Component::*;

    let text = themed(&settings.text, Fg);
    let border = themed(&settings.border, Fg);
    let action = themed(&settings.action, Fg);
    let time = themed_or(&settings.time, &text, Fg);
    let greet = themed_or(&settings.greet, &text, Fg);
    let title = themed_or(&settings.title, &border, Fg);
    let button = themed_or(&settings.button, &action, Fg);

    Theme {
      container: themed(&settings.container, Bg),
      time,
      text,
      border,
      title,
      greet,
      prompt: themed(&settings.prompt, Fg),
      input: themed(&settings.input, Fg),
      action,
      button,
    }
  }

  pub fn of(&self, targets: &[Themed]) -> Style {
    targets
      .iter()
      .fold(Style::default(), |style, target| self.apply(style, target))
  }

  fn apply(&self, style: Style, target: &Themed) -> Style {
    use Themed::*;

    let color = match target {
      Container => &self.container,
      Time => &self.time,
      Text => &self.text,
      Border => &self.border,
      Title => &self.title,
      Greet => &self.greet,
      Prompt => &self.prompt,
      Input => &self.input,
      Action => &self.action,
      ActionButton => &self.button,
    };

    match color {
      Some((component, color)) => match component {
        Component::Fg => style.fg(*color),
        Component::Bg => style.bg(*color),
      },

      None => style,
    }
  }
}

fn themed(setting: &ThemeColor, component: Component) -> Option<(Component, Color)> {
  let ThemeColor::Value(value) = setting else {
    return None;
  };
  Color::from_str(value).ok().map(|color| (component, color))
}

fn themed_or(
  setting: &ThemeColor,
  fallback: &Option<(Component, Color)>,
  component: Component,
) -> Option<(Component, Color)> {
  match setting {
    ThemeColor::Unset => fallback.clone(),
    ThemeColor::Value(_) => themed(setting, component),
    ThemeColor::Clear => None,
  }
}

#[cfg(test)]
mod tests {
  use ratatui::style::Color;

  use super::{Theme, Themed};
  use crate::config::{ThemeColor, ThemeSettings};

  #[test]
  fn theme_settings_distinguish_fallback_from_explicit_clear() {
    let settings = ThemeSettings {
      text: ThemeColor::Value("red".into()),
      time: ThemeColor::Clear,
      border: ThemeColor::Value("blue".into()),
      ..ThemeSettings::default()
    };

    let theme = Theme::from_settings(&settings);

    assert_eq!(theme.of(&[Themed::Text]).fg, Some(Color::Red));
    assert_eq!(theme.of(&[Themed::Greet]).fg, Some(Color::Red));
    assert_eq!(theme.of(&[Themed::Time]).fg, None);
    assert_eq!(theme.of(&[Themed::Title]).fg, Some(Color::Blue));
  }
}
