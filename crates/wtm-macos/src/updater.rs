//! Keeping an installed copy current.
//!
//! The release workflow publishes a stapled `.app` zip and an `appcast.json`
//! beside it; this checks that manifest shortly after launch and every six
//! hours, downloads a newer build, and swaps it in. A restart is what actually
//! switches over — the running image is the old one until then, exactly as
//! Squirrel worked for the Electron app.
//!
//! There are two channels: stable reads the rolling `latest` release, beta
//! reads the prerelease published from the `beta` tag. Which one is followed
//! is a setting; `releases/latest` ignores prereleases, so a beta build is
//! never handed to someone on stable.
//!
//! What is downloaded is checked twice before it replaces anything: its
//! SHA-256 against the manifest, and then its code signature — it must be a
//! notarised Developer ID build signed by the same team as the copy that is
//! running. That second check is the one that matters: it is what stops a
//! tampered or substituted download from being installed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2_foundation::{NSBundle, NSString};
use wtm_core::model::Tone;
use wtm_core::update::{is_newer, Manifest, UpdateChannel, UpdateStatus};
use wtm_core::{Action, App};

/// Long enough after launch that the first git listing is done.
const FIRST_CHECK: Duration = Duration::from_secs(10);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// A stalled download must not hold a thread for the life of the app.
const TIMEOUT_SECS: u64 = 120;

thread_local! {
    /// The version installed and waiting for a restart, if any.
    static READY: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The version waiting for a restart, if an update has been installed.
pub fn ready_version(_mtm: MainThreadMarker) -> Option<String> {
    READY.with(|r| r.borrow().clone())
}

/// Start the update loop. Does nothing unless this is an installed release
/// build: a copy run from `cargo run` or out of `target/` has no business
/// replacing itself, and its placeholder version would think every release is
/// an upgrade.
pub fn start(app: &App, mtm: MainThreadMarker) {
    let Some(install) = installation(mtm) else {
        log::info!("updates: not an installed release build, skipping");
        return;
    };
    log::info!(
        "updates: {} {} — following the {} channel",
        install.bundle.display(),
        install.version,
        app.model().config.update_channel.name()
    );
    schedule(app.clone(), install, FIRST_CHECK);
}

/// Check now, from the menu item. Reports the outcome either way.
pub fn check_now(app: &App, mtm: MainThreadMarker) {
    match installation(mtm) {
        Some(install) => run_check(app.clone(), install, true),

        None => app.dispatch(Action::ShowNotice {
            text: "This build does not update itself. Install the app from the latest release to \
                   get updates."
                .into(),
            tone: Tone::Info,
        }),
    }
}

/// The channel setting changed: read the new channel's feed straight away,
/// so switching does not mean waiting for the next six-hourly check. A build
/// that cannot update itself says nothing — the setting is still worth
/// keeping, it just has no effect until the app is installed from a release.
pub fn channel_changed(app: &App, mtm: MainThreadMarker) {
    if let Some(install) = installation(mtm) {
        run_check(app.clone(), install, true);
    }
}

fn schedule(app: App, install: Installation, delay: Duration) {
    let _ =
        DispatchQueue::main().after(DispatchTime::NOW.time(delay.as_nanos() as i64), move || {
            run_check(app.clone(), install.clone(), false);
            schedule(app, install, INTERVAL);
        });
}

/// The running app, when it is one that can update itself.
#[derive(Clone)]
struct Installation {
    bundle: PathBuf,
    version: String,
    team: String,
}

fn installation(mtm: MainThreadMarker) -> Option<Installation> {
    let _ = mtm;
    let bundle = NSBundle::mainBundle();
    let path = PathBuf::from(bundle.bundlePath().to_string());
    if path.extension()? != "app" {
        return None;
    }
    let version = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|v| v.to_string())?;
    // The tree's placeholder: a build that was never stamped by the release
    // workflow, so it is a local build whatever it is running from.
    if version == "0.1.0" {
        return None;
    }
    Some(Installation {
        team: team_id(&path)?,
        bundle: path,
        version,
    })
}

/// The Team ID a bundle is signed with, which a replacement has to match.
fn team_id(app: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["--display", "--verbose=4"])
        .arg(app)
        .output()
        .ok()?;
    // codesign writes its report to stderr.
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .map(str::to_string)
        .filter(|t| t != "not set")
}

fn run_check(app: App, install: Installation, announce: bool) {
    if READY.with(|r| r.borrow().is_some()) {
        return; // Already installed; waiting for a restart.
    }
    if announce {
        app.dispatch(Action::ShowNotice {
            text: "Checking for updates…".into(),
            tone: Tone::Info,
        });
    }
    // Read the channel here, on the main thread, so the worker carries a
    // decision rather than a handle to the model.
    let channel = app.model().config.update_channel;
    // curl, unzip and codesign all block; none of it belongs on the main
    // thread, and none of it needs the model.
    std::thread::Builder::new()
        .name("wtm-updater".into())
        .spawn(move || {
            let outcome = check_and_install(&install, channel);
            let app = app.clone();
            DispatchQueue::main().exec_async(move || report(&app, outcome, announce));
        })
        .expect("spawn updater thread");
}

fn report(app: &App, outcome: Result<UpdateStatus, String>, announce: bool) {
    match outcome {
        Ok(UpdateStatus::Ready(version)) => {
            READY.with(|r| *r.borrow_mut() = Some(version.clone()));
            app.dispatch(Action::ShowNotice {
                text: format!("Version {version} is installed. Restart to use it."),
                tone: Tone::Info,
            });
            if let Some(mtm) = MainThreadMarker::new() {
                if let Some(c) = crate::controller::controller(mtm) {
                    c.update_became_ready();
                }
            }
        }
        Ok(UpdateStatus::UpToDate) if announce => app.dispatch(Action::ShowNotice {
            text: "You are up to date.".into(),
            tone: Tone::Info,
        }),
        Ok(_) => {}
        Err(message) => {
            log::warn!("updates: {message}");
            if announce {
                app.dispatch(Action::ShowNotice {
                    text: format!("Could not check for updates: {message}"),
                    tone: Tone::Error,
                });
            }
        }
    }
}

fn check_and_install(
    install: &Installation,
    channel: UpdateChannel,
) -> Result<UpdateStatus, String> {
    let manifest = Manifest::parse(&fetch(&channel.feed_url())?)?;
    if !is_newer(&manifest.version, &install.version) {
        log::info!(
            "updates: {} is the latest on {} ({} released)",
            install.version,
            channel.name(),
            manifest.version
        );
        return Ok(UpdateStatus::UpToDate);
    }
    log::info!("updates: {} available", manifest.version);

    let work = tempdir()?;
    let zip = work.join("update.zip");
    download(&manifest.url, &zip)?;
    verify_hash(&zip, &manifest.sha256)?;

    let unpacked = work.join("app");
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    run(
        "/usr/bin/ditto",
        &[
            "-x",
            "-k",
            zip.to_str().unwrap(),
            unpacked.to_str().unwrap(),
        ],
    )?;
    let new_app = std::fs::read_dir(&unpacked)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("the download contained no app")?;

    verify_signature(&new_app, &install.team)?;
    swap(&new_app, &install.bundle)?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(UpdateStatus::Ready(manifest.version))
}

fn fetch(url: &str) -> Result<String, String> {
    let out = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &TIMEOUT_SECS.to_string(),
            url,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    run(
        "/usr/bin/curl",
        &[
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &TIMEOUT_SECS.to_string(),
            "--output",
            to.to_str().ok_or("bad path")?,
            url,
        ],
    )
}

fn verify_hash(file: &Path, expected: &str) -> Result<(), String> {
    let out = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|e| e.to_string())?;
    let actual = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("the download does not match the hash in the manifest".into());
    }
    Ok(())
}

/// The download has to be a notarised Developer ID build from the same team as
/// the running copy. Anything else is refused, whatever the manifest said.
fn verify_signature(app: &Path, team: &str) -> Result<(), String> {
    run(
        "/usr/bin/codesign",
        &[
            "--verify",
            "--deep",
            "--strict",
            app.to_str().ok_or("bad path")?,
        ],
    )
    .map_err(|e| format!("the downloaded app is not properly signed: {e}"))?;

    let downloaded = team_id(app).ok_or("the downloaded app carries no Team ID")?;
    if downloaded != team {
        return Err(format!(
            "the downloaded app is signed by {downloaded}, not {team}"
        ));
    }

    let assessment = Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "exec", "-vv"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    let report = String::from_utf8_lossy(&assessment.stderr);
    if !report.contains("source=Notarized Developer ID") {
        return Err("the downloaded app is not notarised".into());
    }
    Ok(())
}

/// Put `new_app` where the running copy is. The old bundle is moved aside
/// first and only removed once the new one is in place, so a failure leaves
/// something that runs.
fn swap(new_app: &Path, installed: &Path) -> Result<(), String> {
    let parent = installed.parent().ok_or("the app has no parent folder")?;
    let staged = parent.join(".worktree-manager-update.app");
    let old = parent.join(".worktree-manager-previous.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);

    // Copy into the destination folder first: a rename across filesystems
    // fails, and the download lives in a temp directory.
    run(
        "/bin/cp",
        &[
            "-R",
            new_app.to_str().ok_or("bad path")?,
            staged.to_str().ok_or("bad path")?,
        ],
    )
    .map_err(|e| format!("could not write to {}: {e}", parent.display()))?;

    std::fs::rename(installed, &old)
        .map_err(|e| format!("could not move the installed app aside: {e}"))?;
    if let Err(e) = std::fs::rename(&staged, installed) {
        // Put back what was there.
        let _ = std::fs::rename(&old, installed);
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("could not install the update: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Quit and start the copy that was just installed. The shell outlives this
/// process on purpose: `open` has to run once the old app is gone, or it just
/// activates the copy that is still running.
pub fn restart(mtm: MainThreadMarker) {
    let Some(install) = installation(mtm) else {
        return;
    };
    let script = format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; open {app}",
        pid = std::process::id(),
        app = shell_quote(&install.bundle.to_string_lossy()),
    );
    let _ = Command::new("/bin/sh").args(["-c", &script]).spawn();
    objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None);
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn tempdir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("wtm-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(stderr.lines().next().unwrap_or("failed").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_app(at: &Path, marker: &str) {
        std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
        std::fs::write(at.join("Contents/MacOS/worktree-manager"), marker).unwrap();
    }

    #[test]
    fn swap_replaces_the_installed_bundle() {
        let root = std::env::temp_dir().join(format!("wtm-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let installed = root.join("Applications/Worktree Manager.app");
        let downloaded = root.join("download/Worktree Manager.app");
        fake_app(&installed, "old");
        fake_app(&downloaded, "new");

        swap(&downloaded, &installed).unwrap();

        let binary = installed.join("Contents/MacOS/worktree-manager");
        assert_eq!(std::fs::read_to_string(binary).unwrap(), "new");
        // Nothing left over beside it.
        let siblings: Vec<String> = std::fs::read_dir(root.join("Applications"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(siblings, vec!["Worktree Manager.app".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hashes_are_checked_against_the_manifest() {
        let file = std::env::temp_dir().join(format!("wtm-hash-{}", std::process::id()));
        std::fs::write(&file, b"worktree manager").unwrap();
        let expected = "6d5b8b6f5b0d9f4e46a2b0a4c9e0e37b0a8c2e3f4d5a6b7c8d9e0f1a2b3c4d5e";
        assert!(verify_hash(&file, expected).is_err());
        let actual = String::from_utf8(
            Command::new("/usr/bin/shasum")
                .args(["-a", "256"])
                .arg(&file)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let actual = actual.split_whitespace().next().unwrap().to_string();
        assert!(verify_hash(&file, &actual).is_ok());
        assert!(verify_hash(&file, &actual.to_uppercase()).is_ok());
        let _ = std::fs::remove_file(&file);
    }

    /// The gate that decides whether updates run at all, and the one that
    /// decides whether a download is installed, both read `codesign` and
    /// `spctl` output — so they are checked against a real notarised app when
    /// one is on the machine, and against this build, which is ad-hoc signed
    /// and must be refused.
    #[test]
    fn signature_checks_read_the_tools_correctly() {
        let unsigned =
            std::env::temp_dir().join(format!("wtm-unsigned-{}.app", std::process::id()));
        fake_app(&unsigned, "x");
        assert_eq!(team_id(&unsigned), None);
        assert!(verify_signature(&unsigned, "ABCDE12345").is_err());
        let _ = std::fs::remove_dir_all(&unsigned);

        let notarised = Path::new("/Applications/Ghostty.app");
        if !notarised.is_dir() {
            return; // Nothing installed to check against; the rest is above.
        }
        let team = team_id(notarised).expect("a Developer ID app has a Team ID");
        assert!(verify_signature(notarised, &team).is_ok());
        // Signed, notarised, but by someone else: refused.
        assert!(verify_signature(notarised, "NOTTHISTEAM").is_err());
    }

    #[test]
    fn paths_with_spaces_survive_the_relaunch_shell() {
        assert_eq!(
            shell_quote("/Applications/A B.app"),
            "'/Applications/A B.app'"
        );
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
