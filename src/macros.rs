use greetd_ipc::Request;

pub trait SafeDebug {
  fn safe_repr(&self) -> String;
}

impl SafeDebug for Request {
  fn safe_repr(&self) -> String {
    match self {
      msg @ &Request::CancelSession => format!("{msg:?}"),
      msg @ &Request::CreateSession { .. } => format!("{msg:?}"),
      &Request::PostAuthMessageResponse { .. } => "PostAuthMessageResponse".to_string(),
      Request::StartSession { cmd, env } => {
        let environment_keys: Vec<_> = env
          .iter()
          .map(|entry| match entry.split_once('=') {
            Some((key, _)) if valid_environment_key(key) => key,
            _ => "<invalid>",
          })
          .collect();

        format!(
          "StartSession {{ command_arguments: {}, environment_entries: {}, environment_keys: {:?} }}",
          cmd.len(),
          env.len(),
          environment_keys
        )
      },
    }
  }
}

fn valid_environment_key(key: &str) -> bool {
  let mut chars = key.chars();

  matches!(chars.next(), Some('A'..='Z' | 'a'..='z' | '_'))
    && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

macro_rules! text {
  ($greeter:expr, $field:ident) => {{ $greeter.text.$field.clone() }};
}

#[cfg(test)]
mod tests {
  use greetd_ipc::Request;

  use super::SafeDebug;

  #[test]
  fn start_session_debug_output_redacts_commands_and_environment_values() {
    let request = Request::StartSession {
      cmd: vec!["/bin/sh".into(), "--token=hunter2".into()],
      env: vec![
        "DISPLAY=:1".into(),
        "ACCESS_TOKEN=top-secret".into(),
        "malformed-secret".into(),
      ],
    };

    let output = request.safe_repr();

    assert_eq!(
      output,
      "StartSession { command_arguments: 2, environment_entries: 3, environment_keys: [\"DISPLAY\", \"ACCESS_TOKEN\", \"<invalid>\"] }"
    );
    for secret in ["/bin/sh", "hunter2", ":1", "top-secret", "malformed-secret"] {
      assert!(!output.contains(secret));
    }
  }
}
