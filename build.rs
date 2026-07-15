mod build_version;

use std::env;

fn main() {
  let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
  let package_version = env!("CARGO_PKG_VERSION");

  build_version::emit_rerun_directives(manifest_dir.as_ref());
  let version = build_version::resolve(manifest_dir.as_ref(), package_version);

  println!("cargo::rustc-env=VERSION={version}");
  println!(
    "cargo::rustc-env=TARGET={}",
    env::var("TARGET").expect("Cargo sets TARGET")
  );
}
