use std::{
  env,
  ffi::OsStr,
  path::{Path, PathBuf},
  process::Command,
};

const VERSION_OVERRIDE: &str = "TUIGREET_BUILD_VERSION";
const COMMIT_ENVIRONMENTS: [&str; 2] = ["TUIGREET_GIT_SHA", "GITHUB_SHA"];

pub fn resolve(repository: &Path, package_version: &str) -> String {
  if let Some(version) = valid_environment(VERSION_OVERRIDE) {
    return version;
  }

  if let Some(version) =
    describe(repository).and_then(|description| normalize_description(&description, package_version))
  {
    version
  } else if repository.join(".git").exists()
    || git_output(repository, ["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
  {
    fallback_version(package_version)
  } else {
    package_version.to_owned()
  }
}

pub fn emit_rerun_directives(repository: &Path) {
  println!("cargo::rerun-if-env-changed={VERSION_OVERRIDE}");
  for variable in COMMIT_ENVIRONMENTS {
    println!("cargo::rerun-if-env-changed={variable}");
  }

  let Some(git_dir) = git_path(repository, ["rev-parse", "--git-dir"]) else {
    return;
  };
  let common_dir = git_path(repository, ["rev-parse", "--git-common-dir"]).unwrap_or_else(|| git_dir.clone());

  emit_path(repository, &git_dir.join("HEAD"));
  emit_path(repository, &git_dir.join("index"));
  emit_path(repository, &common_dir.join("refs"));
  emit_path(repository, &common_dir.join("packed-refs"));

  if let Some(reference) = git_output(repository, ["symbolic-ref", "-q", "HEAD"])
    && is_safe_reference(&reference)
  {
    emit_path(repository, &common_dir.join(reference));
  }

  // Git metadata does not change for an unstaged edit. Tracking every file known
  // to Git keeps the optional `.dirty` suffix accurate without rerunning for
  // unrelated untracked build output.
  if let Some(files) = git_output_bytes(repository, ["ls-files", "-z"]) {
    for file in files.split(|byte| *byte == 0).filter(|file| !file.is_empty()) {
      if let Ok(file) = std::str::from_utf8(file)
        && !file.contains(['\n', '\r'])
      {
        emit_path(repository, &repository.join(file));
      }
    }
  }
}

fn describe(repository: &Path) -> Option<String> {
  git_output(repository, [
    "describe",
    "--tags",
    "--long",
    "--always",
    "--dirty",
    "--match",
    "[0-9]*.[0-9]*.[0-9]*",
  ])
}

fn normalize_description(description: &str, package_version: &str) -> Option<String> {
  let (description, dirty) = description
    .strip_suffix("-dirty")
    .map_or((description, false), |description| (description, true));

  let mut version = if let Some((tag_and_distance, commit)) = description.rsplit_once("-g") {
    let (tag, distance) = tag_and_distance.rsplit_once('-')?;
    if !valid_tag(tag) || !valid_commit(commit) {
      return None;
    }

    let distance = distance.parse::<u64>().ok()?;
    if distance == 0 {
      tag.to_owned()
    } else {
      format!("{tag}.r{distance}.g{commit}")
    }
  } else if valid_commit(description) {
    format!("{package_version}.dev.g{description}")
  } else {
    return None;
  };

  if dirty {
    version.push_str(".dirty");
  }
  Some(version)
}

fn fallback_version(package_version: &str) -> String {
  let commit = COMMIT_ENVIRONMENTS
    .into_iter()
    .find_map(valid_environment)
    .and_then(|commit| short_commit(&commit));

  commit.map_or_else(
    || package_version.to_owned(),
    |commit| format!("{package_version}.dev.g{commit}"),
  )
}

fn short_commit(commit: &str) -> Option<String> {
  if !valid_commit(commit) {
    return None;
  }

  Some(commit[..commit.len().min(12)].to_owned())
}

fn valid_environment(variable: &str) -> Option<String> {
  env::var(variable)
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty() && !value.contains(['\n', '\r', '\0']))
}

fn valid_tag(tag: &str) -> bool {
  let mut components = tag.split('.');
  let valid = components
    .by_ref()
    .take(3)
    .all(|component| !component.is_empty() && component.chars().all(|character| character.is_ascii_digit()));
  valid && components.next().is_none() && tag.matches('.').count() == 2
}

fn valid_commit(commit: &str) -> bool {
  commit.len() >= 7 && commit.chars().all(|character| character.is_ascii_hexdigit())
}

fn git_path<const N: usize>(repository: &Path, arguments: [&str; N]) -> Option<PathBuf> {
  let path = PathBuf::from(git_output(repository, arguments)?);
  Some(if path.is_absolute() {
    path
  } else {
    repository.join(path)
  })
}

fn git_output<const N: usize>(repository: &Path, arguments: [&str; N]) -> Option<String> {
  let output = git_output_bytes(repository, arguments)?;
  let output = String::from_utf8(output).ok()?;
  let output = output.trim();
  (!output.is_empty()).then(|| output.to_owned())
}

fn git_output_bytes<I, S>(repository: &Path, arguments: I) -> Option<Vec<u8>>
where
  I: IntoIterator<Item = S>,
  S: AsRef<OsStr>,
{
  let output = Command::new("git")
    .args(arguments)
    .current_dir(repository)
    .output()
    .ok()?;
  output.status.success().then_some(output.stdout)
}

fn is_safe_reference(reference: &str) -> bool {
  reference.starts_with("refs/")
    && !reference
      .split('/')
      .any(|component| component.is_empty() || matches!(component, "." | ".."))
}

fn emit_path(repository: &Path, path: &Path) {
  let path = path.strip_prefix(repository).unwrap_or(path);
  if path.exists()
    && let Some(path) = path.to_str()
    && !path.contains(['\n', '\r'])
  {
    println!("cargo::rerun-if-changed={path}");
  }
}
