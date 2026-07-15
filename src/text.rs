#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Text {
  pub title_authenticate: String,
  pub title_command: String,
  pub title_power: String,
  pub title_session: String,
  pub title_users: String,
  pub action_reset: String,
  pub action_command: String,
  pub action_session: String,
  pub action_power: String,
  pub date: String,
  pub select_user: String,
  pub username: String,
  pub wait: String,
  pub failed: String,
  pub greetd_error: String,
  pub new_command: String,
  pub shutdown: String,
  pub reboot: String,
  pub suspend: String,
  pub hibernate: String,
  pub command_missing: String,
  pub command_exited: String,
  pub command_failed: String,
  pub status_command: String,
  pub status_session: String,
  pub status_caps: String,
}

impl Default for Text {
  fn default() -> Self {
    Self {
      title_authenticate: "Authenticate into {hostname}".into(),
      title_command: "Change session command".into(),
      title_power: "Power options".into(),
      title_session: "Change session".into(),
      title_users: "Select a user".into(),
      action_reset: "Reset".into(),
      action_command: "Change command".into(),
      action_session: "Choose session".into(),
      action_power: "Power".into(),
      date: "%a, %d %h %Y - %H:%M".into(),
      select_user: "Press Enter to select a user or start typing...".into(),
      username: "Username:".into(),
      wait: "Please wait...".into(),
      failed: "Authentication failed, please try again.".into(),
      greetd_error: "An error was received from greetd".into(),
      new_command: "New command:".into(),
      shutdown: "Shut down".into(),
      reboot: "Reboot".into(),
      suspend: "Suspend".into(),
      hibernate: "Hibernate".into(),
      command_missing: "No command configured".into(),
      command_exited: "Command exited with".into(),
      command_failed: "Command failed".into(),
      status_command: "CMD".into(),
      status_session: "SESS".into(),
      status_caps: "CAPS LOCK".into(),
    }
  }
}

impl Text {
  pub fn authenticate_title(&self, hostname: &str) -> String {
    self.title_authenticate.replace("{hostname}", hostname)
  }
}
