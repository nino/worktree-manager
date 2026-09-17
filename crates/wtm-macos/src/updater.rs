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
//!
//! A standard account cannot write to `/Applications`, so for it the swap is
//! done by an administrator: the download is held, checked, and put in place
//! only when someone answers the authentication dialog. Nothing is authorised
//! that has not been checked, and it is checked again immediately before the
//! privileged copy, because between the two it sits in a folder this account
//! can write to.

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
    /// The newest version a background check has already said cannot be
    /// installed, so the six-hourly check does not repeat itself.
    static TOLD_UNREPLACEABLE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// A checked download waiting for an administrator to allow it in.
    static PENDING: std::cell::RefCell<Option<Staged>> = const { std::cell::RefCell::new(None) };
    /// An authorised install is under way, so a second press of the button
    /// does not put a second password dialog on screen.
    static INSTALLING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The version waiting for a restart, if an update has been installed.
pub fn ready_version(_mtm: MainThreadMarker) -> Option<String> {
    READY.with(|r| r.borrow().clone())
}

/// The version downloaded and checked but not yet allowed in, if any.
pub fn pending_version(_mtm: MainThreadMarker) -> Option<String> {
    PENDING.with(|p| p.borrow().as_ref().map(|s| s.version.clone()))
}

/// A download that is checked and ready, waiting for the permission that lets
/// it replace the running copy.
#[derive(Clone)]
struct Staged {
    version: String,
    /// The unpacked bundle, still in the work directory.
    app: PathBuf,
    /// Removed once the update is installed.
    work: PathBuf,
    /// Where it goes.
    bundle: PathBuf,
    /// Re-checked against this before anything runs as root.
    team: String,
    /// What the notice says is needed, so the offer can be made again.
    advice: String,
}

/// The one line the notice bar shows for an update that is downloaded and
/// waiting on permission.
fn offer(version: &str, advice: &str) -> String {
    format!("Version {version} is ready to install. {advice}")
}

/// What a check produced: what to tell the user, and the download that is
/// waiting on them, if there is one.
struct Checked {
    status: UpdateStatus,
    staged: Option<Staged>,
}

impl Checked {
    fn just(status: UpdateStatus) -> Self {
        Checked {
            status,
            staged: None,
        }
    }
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
    if let Some(waiting) = PENDING.with(|p| p.borrow().clone()) {
        // Downloaded and waiting for permission. Checking again would clear the
        // work directory it is sitting in — and the notice may have been
        // dismissed since, which used to leave the update unreachable until the
        // next launch, because this is the only thing that offers it.
        app.dispatch(Action::ShowNotice {
            text: offer(&waiting.version, &waiting.advice),
            tone: Tone::Info,
        });
        return;
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
            DispatchQueue::main()
                .exec_async(move || report(&app, outcome, announce, &install.version, channel));
        })
        .expect("spawn updater thread");
}

fn report(
    app: &App,
    outcome: Result<Checked, String>,
    announce: bool,
    running: &str,
    channel: UpdateChannel,
) {
    // The worker did the downloading; the state it produced belongs here, on
    // the thread that owns it.
    let outcome = outcome.map(|checked| {
        if let Some(staged) = checked.staged {
            PENDING.with(|p| *p.borrow_mut() = Some(staged));
        }
        checked.status
    });
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
        Ok(UpdateStatus::NeedsAuthorisation { version, advice }) => {
            log::info!("updates: {version} downloaded, waiting for authorisation");
            app.dispatch(Action::ShowNotice {
                text: offer(&version, &advice),
                tone: Tone::Info,
            });
            // Puts the button in the notice bar; the dialog comes when it is
            // pressed, not out of nowhere while someone is working.
            if let Some(mtm) = MainThreadMarker::new() {
                if let Some(c) = crate::controller::controller(mtm) {
                    c.update_became_ready();
                }
            }
        }
        Ok(UpdateStatus::Unreplaceable { version, advice }) => {
            log::info!("updates: {version} available but not installable: {advice}");
            // Said once per version even for the background check: without it
            // a copy that cannot replace itself would never hear of an update.
            let already_told = TOLD_UNREPLACEABLE
                .with(|t| t.replace(Some(version.clone())) == Some(version.clone()));
            if announce || !already_told {
                app.dispatch(Action::ShowNotice {
                    text: format!("Version {version} is available. {advice}"),
                    tone: Tone::Info,
                });
            }
        }
        Ok(UpdateStatus::UpToDate) if announce => app.dispatch(Action::ShowNotice {
            // Naming the version and channel is what tells someone which
            // build they are on after a restart, without opening About.
            text: format!(
                "You are up to date: {running} is the latest {} build.",
                channel.name()
            ),
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

fn check_and_install(install: &Installation, channel: UpdateChannel) -> Result<Checked, String> {
    let manifest = Manifest::parse(&fetch(&channel.feed_url())?)?;
    if !is_newer(&manifest.version, &install.version) {
        log::info!(
            "updates: {} is the latest on {} ({} released)",
            install.version,
            channel.name(),
            manifest.version
        );
        return Ok(Checked::just(UpdateStatus::UpToDate));
    }
    log::info!("updates: {} available", manifest.version);

    // Before downloading anything: an update that nobody can put in place is
    // news for the user, not a failure to report. A folder this account may
    // not write to is not one of those — it takes an administrator, who is
    // asked once the download has been checked.
    let blocker = replace_blocker(&install.bundle);
    if let Some(blocker) = blocker.as_ref().filter(|b| !b.needs_authorisation()) {
        return Ok(Checked::just(UpdateStatus::Unreplaceable {
            version: manifest.version,
            advice: blocker.advice(),
        }));
    }

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

    if let Some(blocker) = blocker {
        // Checked, and going no further until someone with the rights says so.
        // It stays where it is: nothing has been installed and nothing run.
        return Ok(Checked {
            status: UpdateStatus::NeedsAuthorisation {
                version: manifest.version.clone(),
                advice: blocker.advice(),
            },
            staged: Some(Staged {
                version: manifest.version,
                app: new_app,
                work,
                bundle: install.bundle.clone(),
                team: install.team.clone(),
                advice: blocker.advice(),
            }),
        });
    }

    swap(&new_app, &install.bundle)?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(Checked::just(UpdateStatus::Ready(manifest.version)))
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

/// Why the running copy cannot be swapped for a new one where it is.
#[derive(Debug, PartialEq, Eq)]
enum Blocker {
    /// App Translocation: a quarantined app opened from where it was
    /// downloaded runs from a randomised read-only mount. Moving it in Finder
    /// is what ends that.
    Translocated,
    /// A disk image, or some other read-only volume.
    ReadOnly,
    /// A folder this user may not write to, e.g. /Applications for a
    /// standard account.
    NoPermission(PathBuf),
}

impl Blocker {
    /// Whether permission is the only thing missing. A read-only volume and a
    /// translocated copy cannot be written to by anyone, root included; a
    /// folder this account does not own can be, once an administrator says so.
    fn needs_authorisation(&self) -> bool {
        matches!(self, Blocker::NoPermission(_))
    }

    /// Follows "Version … is available." in the notice bar, or "Version … is
    /// ready to install." for the one an administrator can let in — so it has
    /// to be short enough to read without Details, and to say what the button
    /// is about to ask for.
    fn advice(&self) -> String {
        match self {
            Blocker::Translocated | Blocker::ReadOnly => {
                "Move Worktree Manager to Applications to install it.".into()
            }
            Blocker::NoPermission(folder) => format!(
                "It needs an administrator's permission to change {}.",
                folder.display()
            ),
        }
    }
}

/// Whether `swap` would fail for a reason no retry will fix. Writing a file
/// beside the bundle is the check, because it is exactly what `swap` does;
/// the translocation test comes first only for the sake of a clearer message.
fn replace_blocker(bundle: &Path) -> Option<Blocker> {
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Some(Blocker::Translocated);
    }
    let parent = bundle.parent()?;
    let probe = parent.join(format!(".worktree-manager-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(e) if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem => Some(Blocker::ReadOnly),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Some(Blocker::NoPermission(parent.to_path_buf()))
        }
        // Anything else may be passing; let `swap` report it if it recurs.
        Err(_) => None,
    }
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

/// What the user is told when they answer the authentication dialog with
/// Cancel. Their decision, so it is reported as news rather than an error, and
/// the offer stays up.
const CANCELLED: &str = "Installation was cancelled.";

/// The notice bar's button. An update that is already installed only needs a
/// restart; one still waiting for permission is installed first, which is
/// where the authentication dialog comes from — pressed, never unprompted.
pub fn install_or_restart(app: &App, mtm: MainThreadMarker) {
    let Some(staged) = PENDING.with(|p| p.borrow().clone()) else {
        restart(mtm);
        return;
    };
    if INSTALLING.replace(true) {
        return; // A dialog is already up.
    }
    app.dispatch(Action::ShowNotice {
        text: format!("Installing version {}…", staged.version),
        tone: Tone::Info,
    });
    // osascript blocks for as long as the dialog is on screen, which is as
    // long as the user takes.
    let app = app.clone();
    std::thread::Builder::new()
        .name("wtm-installer".into())
        .spawn(move || {
            let outcome = authorise_and_swap(&staged);
            DispatchQueue::main().exec_async(move || match outcome {
                Ok(()) => {
                    INSTALLING.set(false);
                    let _ = std::fs::remove_dir_all(&staged.work);
                    PENDING.with(|p| *p.borrow_mut() = None);
                    READY.with(|r| *r.borrow_mut() = Some(staged.version.clone()));
                    // They asked for it to be installed, and it is: the old
                    // image is still what is running, so take the restart now
                    // rather than ask a second time.
                    if let Some(mtm) = MainThreadMarker::new() {
                        restart(mtm);
                    }
                }
                Err(message) => {
                    INSTALLING.set(false);
                    let cancelled = message == CANCELLED;
                    if !cancelled {
                        log::warn!("updates: {message}");
                    }
                    // The download stays staged either way, so the button is
                    // still there to try again.
                    app.dispatch(Action::ShowNotice {
                        text: message,
                        tone: if cancelled { Tone::Info } else { Tone::Error },
                    });
                }
            });
        })
        .expect("spawn installer thread");
}

/// Check the staged bundle once more and then have an administrator put it in
/// place. The second check is not ceremony: the bundle has been sitting in a
/// folder this account can write to since the first one, and what follows runs
/// as root.
fn authorise_and_swap(staged: &Staged) -> Result<(), String> {
    verify_signature(&staged.app, &staged.team)?;
    let parent = staged
        .bundle
        .parent()
        .ok_or("the app has no parent folder")?;
    let script = install_script(
        &staged.app,
        &staged.bundle,
        &parent.join(".worktree-manager-update.app"),
        &parent.join(".worktree-manager-previous.app"),
    );
    run_as_administrator(
        &script,
        "Worktree Manager needs permission to install the update.",
    )
}

/// The steps `swap` takes, in the order it takes them, as one shell command:
/// the new bundle is copied in beside the old one, the old one is moved aside,
/// and it is put back if the last move fails — so an interruption leaves an
/// app that still runs. Every path is quoted; this runs as root.
fn install_script(new_app: &Path, installed: &Path, staged: &Path, old: &Path) -> String {
    let new_app = shell_quote(&new_app.to_string_lossy());
    let installed = shell_quote(&installed.to_string_lossy());
    let staged = shell_quote(&staged.to_string_lossy());
    let old = shell_quote(&old.to_string_lossy());
    format!(
        "rm -rf {staged} {old} && cp -R {new_app} {staged} && mv {installed} {old} && \
         {{ mv {staged} {installed} || {{ mv {old} {installed}; rm -rf {staged}; exit 1; }}; }} && \
         rm -rf {old}"
    )
}

/// Run a shell command as an administrator. `osascript` is what puts macOS's
/// own authentication dialog on screen — the same one the Electron app raised
/// — and a standard account can answer it with an administrator's name and
/// password, which is the only way an update reaches a folder that account
/// does not own.
fn run_as_administrator(command: &str, prompt: &str) -> Result<(), String> {
    let script = format!(
        "do shell script {command} with prompt {prompt} with administrator privileges",
        command = applescript_quote(command),
        prompt = applescript_quote(prompt),
    );
    let out = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // -128 is AppleScript for "the user cancelled".
    if stderr.contains("-128") {
        return Err(CANCELLED.into());
    }
    Err(stderr
        .lines()
        .next()
        .unwrap_or("the update could not be installed")
        .trim()
        .to_string())
}

/// A string literal for `osascript -e`. AppleScript escapes backslashes and
/// double quotes and nothing else, so the shell quoting inside — single
/// quotes, from `shell_quote` — passes through untouched.
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
        std::fs::write(at.join("Contents/MacOS/Worktree Manager"), marker).unwrap();
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

        let binary = installed.join("Contents/MacOS/Worktree Manager");
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
    fn a_copy_that_cannot_be_replaced_is_recognised_before_downloading() {
        let root = std::env::temp_dir().join(format!("wtm-blocker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let writable = root.join("Applications/Worktree Manager.app");
        fake_app(&writable, "x");
        assert_eq!(replace_blocker(&writable), None);
        // The probe is not left behind.
        assert_eq!(
            std::fs::read_dir(root.join("Applications"))
                .unwrap()
                .count(),
            1
        );

        let translocated =
            Path::new("/private/var/folders/xx/T/AppTranslocation/888FD9E8/d/Worktree Manager.app");
        assert_eq!(replace_blocker(translocated), Some(Blocker::Translocated));

        let locked = root.join("Locked/Worktree Manager.app");
        fake_app(&locked, "x");
        let folder = root.join("Locked");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(
            replace_blocker(&locked),
            Some(Blocker::NoPermission(folder.clone()))
        );
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
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

    /// Two sentences from two places, and the second has to read as one with
    /// the first — it is the whole of what the notice bar says before someone
    /// is asked for a password.
    #[test]
    fn the_offer_says_what_the_button_will_ask_for() {
        let advice = Blocker::NoPermission(PathBuf::from("/Applications")).advice();
        assert_eq!(
            offer("1.0.126", &advice),
            "Version 1.0.126 is ready to install. \
             It needs an administrator's permission to change /Applications."
        );
    }

    /// The one blocker that is got past rather than reported.
    #[test]
    fn only_a_folder_this_account_cannot_write_to_is_worth_asking_about() {
        assert!(Blocker::NoPermission(PathBuf::from("/Applications")).needs_authorisation());
        assert!(!Blocker::Translocated.needs_authorisation());
        assert!(!Blocker::ReadOnly.needs_authorisation());
    }

    /// This command is handed to root, so it has to do exactly what `swap`
    /// does, and every path in it has to survive a space.
    #[test]
    fn the_administrator_runs_the_same_steps_as_an_ordinary_swap() {
        let script = install_script(
            Path::new("/tmp/wtm-update-1/app/Worktree Manager.app"),
            Path::new("/Applications/Worktree Manager.app"),
            Path::new("/Applications/.worktree-manager-update.app"),
            Path::new("/Applications/.worktree-manager-previous.app"),
        );
        let installed = "'/Applications/Worktree Manager.app'";
        let staged = "'/Applications/.worktree-manager-update.app'";
        let previous = "'/Applications/.worktree-manager-previous.app'";
        assert!(script.contains(&format!(
            "cp -R '/tmp/wtm-update-1/app/Worktree Manager.app' {staged}"
        )));
        // The installed copy is moved aside before anything takes its place,
        // and put back if that move fails.
        let aside = script
            .find(&format!("mv {installed} {previous}"))
            .expect("the installed copy is moved aside");
        let restore = script
            .find(&format!("mv {previous} {installed}"))
            .expect("and put back when the install fails");
        assert!(aside < restore);
        // Nothing is thrown away until the new copy is in place.
        assert!(script.ends_with(&format!("rm -rf {previous}")));
    }

    /// The shell command is a string inside an AppleScript string, so it is
    /// quoted twice; the inner quoting has to come through untouched.
    #[test]
    fn the_command_survives_applescript_quoting() {
        assert_eq!(
            applescript_quote("rm -rf '/Applications/A B.app'"),
            "\"rm -rf '/Applications/A B.app'\""
        );
        assert_eq!(applescript_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(applescript_quote("back\\slash"), "\"back\\\\slash\"");
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
