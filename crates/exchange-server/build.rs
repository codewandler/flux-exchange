use std::process::Command;

const SOURCE_ENV: &str = "FLUX_EXCHANGE_RELEASE_SOURCE_COMMIT";
const BUILD_ENV: &str = "FLUX_EXCHANGE_RELEASE_BUILD_ID";

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")
        .expect("Cargo must declare the target operating system");
    if target_os != "linux" {
        panic!("flux-exchange server supports Linux only; target OS `{target_os}` is unsupported");
    }

    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={BUILD_ENV}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    rerun_for_git_path("HEAD");
    rerun_for_git_path("index");
    if let Ok(reference) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        rerun_for_git_path(&reference);
    }

    let source = std::env::var(SOURCE_ENV).unwrap_or_else(|_| git_commit());
    assert!(
        source.len() == 40
            && source
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{SOURCE_ENV} (or the development git commit) must be 40 lowercase hexadecimal bytes"
    );
    let build = std::env::var(BUILD_ENV).unwrap_or_else(|_| {
        let dirty = git_output(&["status", "--porcelain"]).is_ok_and(|status| !status.is_empty());
        format!("dev-{source}{}", if dirty { "-dirty" } else { "" })
    });
    assert!(
        !build.is_empty()
            && build.len() <= 128
            && build.bytes().all(|byte| (0x20..=0x7e).contains(&byte)),
        "{BUILD_ENV} must be 1..=128 printable ASCII bytes"
    );
    println!("cargo:rustc-env=FLUX_EXCHANGE_COMPILED_SOURCE_COMMIT={source}");
    println!("cargo:rustc-env=FLUX_EXCHANGE_COMPILED_BUILD_ID={build}");
}

fn git_commit() -> String {
    git_output(&["rev-parse", "HEAD"])
        .expect("release identity is absent and git did not provide the development source commit")
}

fn rerun_for_git_path(path: &str) {
    if let Ok(path) = git_output(&["rev-parse", "--git-path", path]) {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn git_output(arguments: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .args(arguments)
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|_| ())?
        .trim()
        .to_owned())
}
