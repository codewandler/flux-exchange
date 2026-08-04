use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const STORE_SETTINGS: [&str; 8] = [
    "FLUX_EXCHANGE_CREDENTIALS",
    "FLUX_EXCHANGE_SETTINGS",
    "FLUX_EXCHANGE_GRANTS",
    "FLUX_EXCHANGE_CONNECTIONS",
    "FLUX_EXCHANGE_CHANNELS",
    "FLUX_EXCHANGE_WORKFLOWS",
    "FLUX_EXCHANGE_AUDIT",
    "FLUX_EXCHANGE_SERVICE_ACCOUNTS",
];

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-x134-{name}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        exchange_host::ensure_private_state_directory(&path)
            .expect("platform owner-only scratch directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_to_final_bind(
    arguments: &[&str],
    environment: impl IntoIterator<Item = (&'static str, String)>,
) -> Output {
    let occupied = TcpListener::bind("127.0.0.1:0").expect("occupied loopback listener");
    let bind = occupied.local_addr().expect("occupied listener address");
    let mut command = Command::new(env!("CARGO_BIN_EXE_flux-exchange"));
    command
        .env_clear()
        .env("USER", "x134-owner")
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "info")
        .env("FLUX_EXCHANGE_BIND", bind.to_string())
        .args(arguments);
    command.envs(environment);
    let output = command.output().expect("real flux-exchange process");
    drop(occupied);
    output
}

fn diagnostics(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn real_dev_flag_binds_the_complete_default_store_set() {
    let scratch = Scratch::new("dev-defaults");
    let root = scratch.path().join("state");
    exchange_host::ensure_private_state_directory(&root)
        .expect("platform owner-only development state root");

    let output = run_to_final_bind(
        &["--dev"],
        [("FLUX_EXCHANGE_STATE", root.display().to_string())],
    );
    let refusal = diagnostics(&output);

    assert!(
        !output.status.success(),
        "the planted final bind must refuse"
    );
    assert!(
        refusal.contains("DEVELOPMENT identity armed") && refusal.contains("\"--dev\""),
        "the real process did not receive --dev:\n{refusal}"
    );
    assert!(
        refusal.contains("cannot listen on"),
        "startup did not reach the planted final bind after opening stores:\n{refusal}"
    );
    for absent_warning in [
        "no credential store is bound",
        "no channel store is bound",
        "no connection-settings store is bound",
        "no grant store is bound",
        "no connection registry is bound",
        "no workflow store is bound",
        "no durable audit journal is bound",
        "no Service Account store is bound",
    ] {
        assert!(
            !refusal.contains(absent_warning),
            "development silently dropped a durable store ({absent_warning}):\n{refusal}"
        );
    }
    for relative in [
        "credentials/store.txt",
        "settings/store.json",
        "grants/store.json",
        "connections/store.json",
        "channels/store.json",
        "workflows",
        "audit/events.sqlite3",
        "service-accounts/store.json",
    ] {
        let expected = root.join(relative);
        assert!(
            refusal.contains(&expected.display().to_string()),
            "startup did not report the bound default {}:\n{refusal}",
            expected.display()
        );
    }
}

#[test]
fn an_empty_explicit_dev_override_refuses_instead_of_disappearing() {
    let scratch = Scratch::new("empty-override");
    let root = scratch.path().join("state");

    let output = run_to_final_bind(
        &["--dev"],
        [
            ("FLUX_EXCHANGE_STATE", root.display().to_string()),
            ("FLUX_EXCHANGE_CREDENTIALS", String::new()),
        ],
    );
    let refusal = diagnostics(&output);

    assert!(!output.status.success());
    assert!(
        refusal.contains("FLUX_EXCHANGE_CREDENTIALS is set but empty"),
        "the explicit stale override was not refused by name:\n{refusal}"
    );
    assert!(
        !root.exists(),
        "development defaults were created after an explicit override refusal"
    );
}

#[cfg(unix)]
#[test]
fn a_credential_directly_below_tmp_refuses_without_repair() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = Scratch::new("tmp-state-root");
    let state = scratch.path().join("state");
    std::fs::create_dir(&state).expect("state root");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .expect("private state root");
    let direct = std::env::temp_dir().join(format!(
        "flux-secret-x134-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    assert!(
        !direct.exists(),
        "credential fixture path unexpectedly exists"
    );
    let tmp = direct.parent().expect("temporary directory");
    let mode_before = std::fs::metadata(tmp)
        .expect("temporary directory metadata")
        .permissions()
        .mode()
        & 0o7777;

    let output = run_to_final_bind(
        &["--dev"],
        [
            ("FLUX_EXCHANGE_STATE", state.display().to_string()),
            ("FLUX_EXCHANGE_CREDENTIALS", direct.display().to_string()),
        ],
    );
    let refusal = diagnostics(&output);
    let mode_after = std::fs::metadata(tmp)
        .expect("temporary directory after refusal")
        .permissions()
        .mode()
        & 0o7777;

    assert!(!output.status.success());
    assert!(refusal.contains(&direct.display().to_string()), "{refusal}");
    assert!(refusal.contains("private child directory"), "{refusal}");
    assert!(
        refusal.contains("Exchange did not change the existing metadata"),
        "{refusal}"
    );
    assert_eq!(
        mode_after, mode_before,
        "startup repaired the shared parent"
    );
    assert!(
        !direct.exists(),
        "startup created the refused credential file"
    );
}

#[test]
fn a_safe_explicit_dev_store_keeps_every_unset_sibling_default() {
    let scratch = Scratch::new("safe-explicit");
    let state = scratch.path().join("state");
    let explicit_parent = scratch.path().join("operator-selected");
    exchange_host::ensure_private_state_directory(&state)
        .expect("platform owner-only default state root");
    exchange_host::ensure_private_state_directory(&explicit_parent)
        .expect("platform owner-only explicit credential parent");
    let explicit = explicit_parent.join("credential-store");

    let output = run_to_final_bind(
        &["--dev"],
        [
            ("FLUX_EXCHANGE_STATE", state.display().to_string()),
            ("FLUX_EXCHANGE_CREDENTIALS", explicit.display().to_string()),
        ],
    );
    let refusal = diagnostics(&output);

    assert!(
        !output.status.success(),
        "the planted final bind must refuse"
    );
    assert!(refusal.contains("cannot listen on"), "{refusal}");
    assert!(
        refusal.contains(&explicit.display().to_string()),
        "{refusal}"
    );
    assert!(explicit.exists(), "the safe explicit store was not opened");
    for relative in [
        "settings/store.json",
        "grants/store.json",
        "connections/store.json",
        "channels/store.json",
        "workflows",
        "audit/events.sqlite3",
        "service-accounts/store.json",
    ] {
        let expected = state.join(relative);
        assert!(
            refusal.contains(&expected.display().to_string()),
            "unset sibling did not receive its development default {}:\n{refusal}",
            expected.display()
        );
    }
}

#[test]
fn configured_non_dev_partial_store_set_enumerates_every_missing_sibling() {
    let scratch = Scratch::new("partial-production");
    let credential = scratch.path().join("credentials/store");

    let output = run_to_final_bind(
        &[],
        [(
            "FLUX_EXCHANGE_CREDENTIALS",
            credential.display().to_string(),
        )],
    );
    let refusal = diagnostics(&output);

    assert!(!output.status.success());
    assert!(
        refusal.contains("persistent local state is all-or-nothing"),
        "{refusal}"
    );
    assert!(
        refusal.contains("configured FLUX_EXCHANGE_CREDENTIALS"),
        "{refusal}"
    );
    for setting in STORE_SETTINGS.into_iter().skip(1) {
        assert!(
            refusal.contains(setting),
            "partial production refusal omitted {setting}:\n{refusal}"
        );
    }
    assert!(
        !credential.exists(),
        "partial production configuration opened the one named store"
    );
}

#[test]
fn production_root_discovery_does_not_consult_inherited_home_variables() {
    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/local_state.rs"))
            .expect("local-state composition source");
    let discovery = source
        .split("fn conventional_root()")
        .nth(1)
        .and_then(|tail| tail.split("fn ensure_owner_only_root").next())
        .expect("conventional-root implementation");

    for inherited in ["HOME", "XDG_STATE_HOME", "USERPROFILE", "LOCALAPPDATA"] {
        assert!(
            !discovery.contains(&format!("\"{inherited}\"")),
            "production root discovery still trusts inherited {inherited}:\n{discovery}"
        );
    }
    #[cfg(unix)]
    {
        assert!(discovery.contains("getpwuid_r"), "{discovery}");
        assert!(discovery.contains("geteuid"), "{discovery}");
    }
    #[cfg(windows)]
    assert!(discovery.contains("SHGetKnownFolderPath"), "{discovery}");
}

#[cfg(unix)]
#[test]
fn configured_state_root_refuses_a_symlinked_ancestor_without_repair() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let scratch = Scratch::new("symlink-ancestor");
    let real = scratch.path().join("real");
    let link = scratch.path().join("linked");
    std::fs::create_dir(&real).expect("real directory");
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700))
        .expect("owner-only real directory");
    symlink(&real, &link).expect("symlinked ancestor");
    let requested = link.join("state");

    let output = run_to_final_bind(
        &[],
        [
            ("FLUX_EXCHANGE_STATE", requested.display().to_string()),
            ("RUST_LOG", "warn".to_owned()),
        ],
    );
    let refusal = diagnostics(&output);

    assert!(!output.status.success());
    assert!(
        refusal.contains("symlink") && refusal.contains(&link.display().to_string()),
        "symlinked production ancestry was not refused precisely:\n{refusal}"
    );
    assert!(
        !real.join("state").exists(),
        "startup followed and populated the symlinked ancestor"
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("symlink after refusal")
            .file_type()
            .is_symlink(),
        "startup repaired or replaced the symlink"
    );
}

#[cfg(unix)]
#[test]
fn configured_state_root_refuses_an_untrusted_writable_ancestor_without_repair() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = Scratch::new("writable-ancestor");
    let writable = scratch.path().join("writable");
    let state = writable.join("state");
    std::fs::create_dir(&writable).expect("writable ancestor");
    std::fs::create_dir(&state).expect("private leaf");
    std::fs::set_permissions(&writable, std::fs::Permissions::from_mode(0o777))
        .expect("untrusted-writable ancestor");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .expect("owner-only leaf");

    let output = run_to_final_bind(
        &[],
        [
            ("FLUX_EXCHANGE_STATE", state.display().to_string()),
            ("RUST_LOG", "warn".to_owned()),
        ],
    );
    let refusal = diagnostics(&output);
    let mode_after = std::fs::metadata(&writable)
        .expect("writable ancestor after refusal")
        .permissions()
        .mode()
        & 0o777;

    assert!(!output.status.success());
    assert!(
        refusal.contains("refusing local state root")
            && refusal.contains(&writable.display().to_string())
            && refusal.contains("writable"),
        "untrusted-writable ancestry was not refused precisely:\n{refusal}"
    );
    assert_eq!(mode_after, 0o777, "startup repaired the unsafe ancestor");
    assert!(
        !state.join("credentials").exists(),
        "startup opened stores below the unsafe ancestry"
    );
}
