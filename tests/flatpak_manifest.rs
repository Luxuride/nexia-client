//! Guards the release manifest against the sandbox permissions the app depends on.
//!
//! Both concerns below only fail at runtime, inside the sandbox, where the rest of
//! the suite cannot see them: system-account discovery silently finds nothing when
//! the session bus name is not shared, and the virtual filesystem silently fails to
//! mount when `/dev/fuse` is absent or when no `fusermount3` is reachable.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> Result<String, Box<dyn Error>> {
    let path = manifest_dir().join("flatpak/eu.luxuride.nexia.yml");
    Ok(fs::read_to_string(path)?)
}

/// Collects the `finish-args` list entries of the manifest.
fn finish_args(manifest: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut in_section = false;

    for line in manifest.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("finish-args:") {
            in_section = rest.trim().is_empty();
            continue;
        }
        if !in_section {
            continue;
        }
        // The section ends at the next top-level key.
        if !line.starts_with(' ') && !line.starts_with('-') && !trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.trim_start().strip_prefix("- ") {
            args.push(value.trim().to_string());
        }
    }

    args
}

fn shim_path() -> PathBuf {
    manifest_dir().join("flatpak/fusermount3")
}

#[test]
fn finish_args_share_the_gnome_online_accounts_bus_name() -> Result<(), Box<dyn Error>> {
    let args = finish_args(&manifest()?);
    assert!(
        args.iter()
            .any(|a| a == "--talk-name=org.gnome.OnlineAccounts"),
        "finish-args must contain --talk-name=org.gnome.OnlineAccounts so \
         system-account discovery can query GOA; got {args:?}"
    );
    Ok(())
}

#[test]
fn finish_args_expose_the_fuse_device() -> Result<(), Box<dyn Error>> {
    let args = finish_args(&manifest()?);
    // Flatpak has no dedicated `--device=fuse`, and `/dev/fuse` cannot be shared
    // with `--filesystem` (it rejects character devices), so `all` is the only
    // way to get an openable /dev/fuse inside the sandbox.
    assert!(
        args.iter().any(|a| a == "--device=all"),
        "finish-args must contain --device=all so /dev/fuse exists in the sandbox; \
         got {args:?}"
    );
    Ok(())
}

#[test]
fn finish_args_allow_host_spawn_for_the_fuse_shim() -> Result<(), Box<dyn Error>> {
    let args = finish_args(&manifest()?);
    assert!(
        args.iter()
            .any(|a| a == "--talk-name=org.freedesktop.Flatpak"),
        "finish-args must contain --talk-name=org.freedesktop.Flatpak so the \
         fusermount3 shim can reach the host helper; got {args:?}"
    );
    Ok(())
}

#[test]
fn finish_args_do_not_grant_host_keyring_access() -> Result<(), Box<dyn Error>> {
    let args = finish_args(&manifest()?);
    // Inside a sandbox `oo7` detects /.flatpak-info and switches to its file
    // backend sealed by org.freedesktop.portal.Secret, so the keyring stays in
    // $XDG_DATA_HOME/keyrings. Reaching the host Secret Service would expose
    // every secret in the user's keyring to this app for no functional gain.
    for forbidden in [
        "--talk-name=org.freedesktop.secrets",
        "--talk-name=org.kde.ksecretservice",
        "--filesystem=xdg-run/keyring",
    ] {
        assert!(
            !args.iter().any(|a| a == forbidden),
            "finish-args must not contain {forbidden}: oo7 uses its sandboxed \
             file backend instead; got {args:?}"
        );
    }
    Ok(())
}

#[test]
fn manifest_installs_the_fusermount_shim_on_path() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    assert!(
        manifest.contains("/bin/fusermount3"),
        "the manifest must install the fusermount3 shim into $FLATPAK_DEST/bin so \
         `fuser`'s PATH probe finds it"
    );
    Ok(())
}

#[test]
fn fuse_shim_delegates_to_the_host_helper() -> Result<(), Box<dyn Error>> {
    let path = shim_path();
    let script = fs::read_to_string(&path)?;

    assert!(
        script.starts_with("#!/bin/sh"),
        "the shim must be a POSIX shell script so it runs inside the runtime"
    );
    assert!(
        script.contains("flatpak-spawn --host"),
        "the shim must re-execute the host helper through flatpak-spawn"
    );
    assert!(
        script.contains("_FUSE_COMMFD"),
        "the shim must forward _FUSE_COMMFD, or the helper cannot hand the \
         /dev/fuse descriptor back into the sandbox"
    );
    assert!(
        script.contains("--forward-fd"),
        "the shim must forward the communication fd to the host helper"
    );
    Ok(())
}

#[test]
fn fuse_shim_is_executable() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let path = shim_path();
    let mode = fs::metadata(&path)?.permissions().mode();
    assert!(
        mode & 0o111 != 0,
        "{} must carry an executable bit in the repository, or the installed \
         copy is not runnable",
        path.display()
    );
    Ok(())
}

#[test]
fn fuse_shim_has_no_crlf_line_endings() -> Result<(), Box<dyn Error>> {
    let script = fs::read_to_string(shim_path())?;
    assert!(
        !script.contains('\r'),
        "CRLF line endings break the `#!/bin/sh` shebang inside the runtime"
    );
    Ok(())
}
