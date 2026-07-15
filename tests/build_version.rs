#[allow(dead_code)]
#[path = "../build_version.rs"]
mod build_version;

use std::{fs, process::Command};

use tempfile::TempDir;

const PACKAGE_VERSION: &str = "0.10.1";

#[test]
fn exact_tag_uses_the_package_version_shape() {
  let repository = repository();
  git(repository.path(), ["tag", "--no-sign", PACKAGE_VERSION]);

  assert_eq!(
    build_version::resolve(repository.path(), PACKAGE_VERSION),
    PACKAGE_VERSION
  );
}

#[test]
fn commit_after_tag_includes_the_distance_and_commit() {
  let repository = repository();
  git(repository.path(), ["tag", "--no-sign", PACKAGE_VERSION]);
  git(repository.path(), ["commit", "--quiet", "--allow-empty", "-m", "next"]);

  let version = build_version::resolve(repository.path(), PACKAGE_VERSION);
  assert!(version.starts_with("0.10.1.r1.g"), "{version}");
  assert!(!version.ends_with(".dirty"), "{version}");
}

#[test]
fn tracked_edits_are_marked_dirty() {
  let repository = repository();
  git(repository.path(), ["tag", "--no-sign", PACKAGE_VERSION]);
  fs::write(repository.path().join("tracked"), "changed\n").unwrap();

  assert_eq!(
    build_version::resolve(repository.path(), PACKAGE_VERSION),
    format!("{PACKAGE_VERSION}.dirty")
  );
}

#[test]
fn repository_without_tags_has_a_nonempty_development_version() {
  let repository = repository();

  let version = build_version::resolve(repository.path(), PACKAGE_VERSION);
  assert!(version.starts_with("0.10.1.dev.g"), "{version}");
  assert!(version.len() > "0.10.1.dev.g".len(), "{version}");
}

#[test]
fn source_archive_falls_back_to_the_package_version() {
  let directory = TempDir::new().unwrap();

  assert_eq!(
    build_version::resolve(directory.path(), PACKAGE_VERSION),
    PACKAGE_VERSION
  );
}

fn repository() -> TempDir {
  let repository = TempDir::new().unwrap();
  git(repository.path(), ["init", "--quiet"]);
  git(repository.path(), ["config", "user.name", "Test"]);
  git(repository.path(), ["config", "user.email", "test@example.invalid"]);
  git(repository.path(), ["config", "commit.gpgSign", "false"]);
  git(repository.path(), ["config", "tag.gpgSign", "false"]);
  fs::write(repository.path().join("tracked"), "initial\n").unwrap();
  git(repository.path(), ["add", "tracked"]);
  git(repository.path(), ["commit", "--quiet", "-m", "initial"]);
  repository
}

fn git<const N: usize>(repository: &std::path::Path, arguments: [&str; N]) {
  let status = Command::new("git")
    .args(arguments)
    .current_dir(repository)
    .status()
    .unwrap();
  assert!(status.success());
}
