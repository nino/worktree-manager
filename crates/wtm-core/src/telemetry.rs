//! Opt-in error reports, sent to PostHog.
//!
//! PostHog has no Rust SDK worth taking on, and its capture API is one JSON
//! POST, so this is hand-rolled: [`event_body`] builds the body,
//! [`RateLimiter`] keeps a crash loop or a broken repo from spending the free
//! tier, [`scrub`] takes the home folder, URL credentials and email addresses
//! out of a message, and [`Reporter`] decides whether anything goes at all.
//!
//! Nothing is sent unless every one of these holds: the user turned on "Send
//! error reports" in Settings, this is a release build, and the build was
//! given `POSTHOG_KEY` and `POSTHOG_HOST` (the release workflow sets them;
//! a `phc_` key is public by design, so they are not secrets). The
//! anonymous id is a random UUID written beside the config the first time an
//! event is actually sent, and deleted when the setting is turned off.
//!
//! Besides crashes and the errors the app shows, the warnings and errors this
//! app's own crates log go too ([`forward`]): they are where the app says it
//! could not do something the user never sees fail, such as saving the
//! snapshot or listing a repo's branches, and are what a later look at
//! PostHog has to go on.
//!
//! Every event sets `$process_person_profile: false`, so PostHog makes no
//! person profiles, and there is no session replay: this is a desktop app
//! with no web view to record.
//!
//! The POST is made by the system's `curl` (in `/usr/bin` on macOS, in
//! System32 since Windows 10 1803, and on nearly every Linux), as the macOS
//! updater already does for its downloads. A Rust HTTP client would mean a
//! TLS stack in the tree, and the usual ones (ring, aws-lc, OpenSSL) compile
//! C for the target, which `cargo check --target` from Linux cannot do. A
//! machine without curl only logs that the report could not be sent.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{info, warn};
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{json, Map, Value};

/// The version of this build: what the release workflow stamps
/// (`WTM_VERSION`, set by `scripts/bundle.sh --version`), else the
/// workspace's placeholder.
pub const VERSION: &str = match option_env!("WTM_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// At most this many events leave in any rolling hour.
pub const MAX_PER_HOUR: usize = 30;

/// Logged warnings have their own, smaller allowance, so that a laptop
/// offline for an hour, whose every background fetch warns, cannot use up
/// the errors' one.
pub const MAX_LOGGED_PER_HOUR: usize = 10;

/// How long a panic report may hold up the crash.
const PANIC_WAIT: Duration = Duration::from_secs(2);

/// How long any other report may take before curl gives up.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// A message is cut to this many characters: enough for git's stderr, and a
/// bound on what a runaway message can carry.
const MAX_MESSAGE: usize = 1000;

// MARK: Keys

/// Where events go: a PostHog project key and its ingestion host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub key: String,
    pub host: String,
}

/// The keys compiled into this build, if it may send at all. Debug builds
/// never do, so a `cargo run` with the variables set still sends nothing.
pub fn compiled_in() -> Option<Keys> {
    if cfg!(debug_assertions) {
        return None;
    }
    keys_from(option_env!("POSTHOG_KEY"), option_env!("POSTHOG_HOST"))
}

/// Both values present and non-blank, and the host on HTTPS (the key would
/// otherwise travel in the clear).
pub fn keys_from(key: Option<&str>, host: Option<&str>) -> Option<Keys> {
    let key = key?.trim();
    let host = host?.trim().trim_end_matches('/');
    if key.is_empty() || !host.starts_with("https://") || host.len() == "https://".len() {
        return None;
    }
    Some(Keys {
        key: key.to_string(),
        host: host.to_string(),
    })
}

/// PostHog's capture endpoint on `host`.
pub fn endpoint(host: &str) -> String {
    format!("{}/i/v0/e/", host.trim_end_matches('/'))
}

// MARK: Payload

/// What every event carries besides its own properties. `$os` uses
/// PostHog's own names so its filters line up.
pub fn common_properties() -> Map<String, Value> {
    let os = match std::env::consts::OS {
        "macos" => "Mac OS X",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    let mut p = Map::new();
    p.insert("$process_person_profile".into(), Value::Bool(false));
    p.insert("$lib".into(), "worktree-manager".into());
    // Which app sent it: all of Nino's apps share one PostHog project.
    p.insert("app".into(), "worktree-manager".into());
    p.insert("$app_version".into(), VERSION.into());
    p.insert("$os".into(), os.into());
    p.insert("arch".into(), std::env::consts::ARCH.into());
    p
}

/// The properties of an `$exception` event.
pub fn exception_properties(kind: &str, message: &str, handled: bool) -> Map<String, Value> {
    let mut p = Map::new();
    p.insert(
        "$exception_list".into(),
        json!([{
            "type": kind,
            "value": message,
            "mechanism": { "handled": handled, "synthetic": false },
        }]),
    );
    p.insert("$exception_level".into(), "error".into());
    p
}

/// One capture request's body. `properties` win over the common ones of the
/// same name, except that no event may turn person profiles back on.
pub fn event_body(
    key: &str,
    event: &str,
    distinct_id: &str,
    properties: Map<String, Value>,
    at: SystemTime,
) -> Value {
    let mut all = common_properties();
    all.extend(properties);
    all.insert("$process_person_profile".into(), Value::Bool(false));
    json!({
        "api_key": key,
        "event": event,
        "distinct_id": distinct_id,
        "properties": all,
        "timestamp": rfc3339(at),
    })
}

/// A UTC timestamp, `2026-10-17T09:30:00.123Z`.
pub fn rfc3339(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days-to-civil, for days since 1970-01-01.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// A short, stable name for an error message, which PostHog groups by: the
/// app's notices read `Push failed: <git's words>`, so the part before the
/// colon names what went wrong without the details that vary.
pub fn exception_type(message: &str) -> String {
    match message.split_once(": ") {
        Some((head, _))
            if !head.is_empty()
                && head.chars().count() <= 40
                && !head.contains(['/', '\\', '\'', '"']) =>
        {
            head.to_string()
        }
        _ => "Error".to_string(),
    }
}

// MARK: Scrubbing

/// `message` with what would identify the user taken out: the home folder
/// becomes `~` (in either slash direction, since Windows messages use both),
/// a URL's `user:token@` goes, and email addresses become `<email>`. Cut to
/// [`MAX_MESSAGE`] characters.
pub fn scrub(message: &str, home: &str) -> String {
    static USERINFO: OnceLock<Regex> = OnceLock::new();
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    let mut text = message.to_string();
    let home = home.trim_end_matches(['/', '\\']);
    if !home.is_empty() {
        let back = home.replace('/', "\\");
        text = replace_home(&text, home);
        if back != home {
            text = replace_home(&text, &back);
        }
    }
    let userinfo = USERINFO
        .get_or_init(|| Regex::new(r"([A-Za-z][A-Za-z0-9+.-]*://)[^/@\s]+@").expect("regex"));
    text = userinfo.replace_all(&text, "$1").into_owned();
    let email = EMAIL.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}")
            .expect("regex")
    });
    text = email
        .replace_all(&text, |c: &regex::Captures| {
            // `git@github.com:owner/repo` is an SSH remote, not an address.
            if c[0].starts_with("git@") {
                c[0].to_string()
            } else {
                "<email>".to_string()
            }
        })
        .into_owned();
    if text.chars().count() > MAX_MESSAGE {
        text = text.chars().take(MAX_MESSAGE).collect::<String>() + "…";
    }
    text
}

/// Every `home` in `text` that is a whole path component (not `/home/ada`
/// inside `/home/adam`) as `~`.
fn replace_home(text: &str, home: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(home) {
        let after = &rest[i + home.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.')));
        out.push_str(&rest[..i]);
        out.push_str(if whole { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

// MARK: Rate limiting

/// At most `max` events in any rolling `window`, and the same event (by
/// fingerprint) once per window: a repo that fails to list fails at every
/// refresh, and one report says as much as forty.
#[derive(Debug)]
pub struct RateLimiter {
    max: usize,
    window: Duration,
    sent: VecDeque<Instant>,
    seen: HashMap<String, Instant>,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        RateLimiter {
            max,
            window,
            sent: VecDeque::new(),
            seen: HashMap::new(),
        }
    }

    /// Whether an event with this fingerprint may go now; if so, it is
    /// counted.
    pub fn allow(&mut self, fingerprint: &str, now: Instant) -> bool {
        let window = self.window;
        let fresh = |t: &Instant| now.saturating_duration_since(*t) < window;
        while self.sent.front().is_some_and(|t| !fresh(t)) {
            self.sent.pop_front();
        }
        self.seen.retain(|_, t| fresh(t));
        if self.sent.len() >= self.max || self.seen.contains_key(fingerprint) {
            return false;
        }
        self.sent.push_back(now);
        self.seen.insert(fingerprint.to_string(), now);
        true
    }
}

// MARK: Transport

/// Sends one request body. Blocking; [`Reporter`] calls it off the UI thread.
pub trait Transport: Send + Sync + 'static {
    fn post(&self, url: &str, body: &[u8], timeout: Duration) -> Result<(), String>;
}

/// [`Transport`] through the system's `curl`, with the body on stdin so the
/// key never shows in a process listing.
pub struct Curl {
    configure: Box<dyn Fn(&mut Command) + Send + Sync>,
}

impl Curl {
    /// `configure` adjusts the process before it starts: on Windows, so
    /// that it opens no console window.
    pub fn new(configure: impl Fn(&mut Command) + Send + Sync + 'static) -> Self {
        Curl {
            configure: Box::new(configure),
        }
    }
}

/// curl's arguments for one POST of a JSON body read from stdin.
pub fn curl_args(url: &str, timeout: Duration) -> Vec<String> {
    let secs = timeout.as_secs().max(1).to_string();
    [
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        &secs,
        "--request",
        "POST",
        "--header",
        "Content-Type: application/json",
        "--data-binary",
        "@-",
        "--output",
        // Nothing in PostHog's answer is needed, and curl's stdout is not
        // read: an unread pipe that fills would stall it.
        if cfg!(windows) { "NUL" } else { "/dev/null" },
        url,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Transport for Curl {
    fn post(&self, url: &str, body: &[u8], timeout: Duration) -> Result<(), String> {
        let mut command = Command::new("curl");
        command
            .args(curl_args(url, timeout))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        (self.configure)(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A curl that gave up before reading (no network) closes the
            // pipe; its exit status below says why, and waiting for it is
            // what reaps the process.
            let _ = stdin.write_all(body);
        }
        // curl's own --max-time is the real limit; this is the backstop for
        // a curl that hangs before it gets that far.
        let deadline = Instant::now() + timeout + Duration::from_millis(500);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    let mut err = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut err);
                    }
                    return Err(format!("curl {status}: {}", err.trim()));
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timed out".into());
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

// MARK: Reporter

/// Decides what is sent, and sends it off the caller's thread.
///
/// Cheap to call from anywhere, a panic hook included: the opt-in is an
/// atomic, never the config's lock, and the panic path only ever tries its
/// locks.
pub struct Reporter {
    keys: Option<Keys>,
    enabled: AtomicBool,
    /// `set_enabled` has been called once: the first call always clears a
    /// leftover id, later ones only when the setting goes from on to off.
    synced: AtomicBool,
    id_path: PathBuf,
    id: Mutex<Option<String>>,
    limiter: Mutex<RateLimiter>,
    log_limiter: Mutex<RateLimiter>,
    home: String,
    transport: Arc<dyn Transport>,
}

impl Reporter {
    /// `id_path` is where the anonymous id is kept, `home` what [`scrub`]
    /// turns into `~`. Starts off; the app turns it on from the config.
    pub fn new(
        keys: Option<Keys>,
        id_path: PathBuf,
        home: String,
        transport: Arc<dyn Transport>,
    ) -> Self {
        Reporter {
            keys,
            enabled: AtomicBool::new(false),
            synced: AtomicBool::new(false),
            id_path,
            id: Mutex::new(None),
            limiter: Mutex::new(RateLimiter::new(MAX_PER_HOUR, Duration::from_secs(3600))),
            log_limiter: Mutex::new(RateLimiter::new(
                MAX_LOGGED_PER_HOUR,
                Duration::from_secs(3600),
            )),
            home,
            transport,
        }
    }

    /// Follow the setting. Turning it off forgets the anonymous id, so
    /// turning it on again starts as someone new. Called on every config
    /// change, so it touches the disk only when the setting changes.
    pub fn set_enabled(&self, on: bool) {
        let was = self.enabled.swap(on, Ordering::AcqRel);
        let first = !self.synced.swap(true, Ordering::AcqRel);
        if on && !was && self.keys.is_some() {
            info!("error reports are on");
            // At launch, `started` follows. Turned on while running, this is
            // the event that shows reports arrive without a restart.
            if !first {
                // Each time, not once an hour: turning reports back on
                // starts a new anonymous id, which is worth seeing arrive.
                let nonce = uuid::Uuid::new_v4().to_string();
                self.send(
                    "error reports turned on",
                    Map::new(),
                    &format!("error reports turned on\n{nonce}"),
                    &self.limiter,
                    false,
                );
            }
        }
        if !on && (was || first) {
            // Under the id's lock, which `distinct_id` checks the setting
            // inside: a report already on its way cannot write the id back
            // after this.
            let mut slot = self.id.lock();
            *slot = None;
            match std::fs::remove_file(&self.id_path) {
                Ok(()) => info!("error reports are off; forgot the anonymous id"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => warn!("cannot remove {}: {e}", self.id_path.display()),
            }
        }
    }

    /// Whether anything would be sent now.
    pub fn is_on(&self) -> bool {
        self.active().is_some()
    }

    fn active(&self) -> Option<&Keys> {
        self.keys
            .as_ref()
            .filter(|_| self.enabled.load(Ordering::Acquire))
    }

    /// The app has started.
    pub fn started(&self) {
        self.send(
            "app started",
            Map::new(),
            "app started",
            &self.limiter,
            false,
        );
    }

    /// An error the user was shown.
    pub fn error(&self, message: &str) {
        if self.active().is_none() {
            return;
        }
        let message = scrub(message, &self.home);
        let kind = exception_type(&message);
        let fingerprint = format!("{kind}\n{message}");
        self.send(
            "$exception",
            exception_properties(&kind, &message, true),
            &fingerprint,
            &self.limiter,
            false,
        );
    }

    /// A warning or error this app logged ([`forward`]). Grouped by the
    /// module that logged it, since a logged message has no `Push failed:`
    /// head to group by.
    pub fn log(&self, level: log::Level, target: &str, message: &str) {
        if !self.is_on() {
            return;
        }
        let message = scrub(message, &self.home);
        let mut props = exception_properties(target, &message, true);
        let level = if level == log::Level::Error {
            "error"
        } else {
            "warning"
        };
        props.insert("$exception_level".into(), level.into());
        props.insert("log_target".into(), target.into());
        let fingerprint = format!("{target}\n{message}");
        // Errors are rare and serious enough to share the errors' allowance
        // rather than wait behind routine warnings.
        let limiter = if level == "error" {
            &self.limiter
        } else {
            &self.log_limiter
        };
        self.send("$exception", props, &fingerprint, limiter, false);
    }

    /// A panic, from the panic hook: sent before returning, for at most
    /// [`PANIC_WAIT`], since the process is about to end.
    pub fn panic(&self, message: &str, location: Option<String>, thread: Option<&str>) {
        if self.active().is_none() {
            return;
        }
        let message = scrub(message, &self.home);
        let value = match &location {
            Some(at) => format!("{message} (at {})", scrub(at, &self.home)),
            None => message,
        };
        let mut props = exception_properties("panic", &value, false);
        if let Some(thread) = thread {
            props.insert("thread".into(), thread.into());
        }
        self.send("$exception", props, &value, &self.limiter, true);
    }

    fn send(
        &self,
        event: &str,
        props: Map<String, Value>,
        fingerprint: &str,
        limiter: &Mutex<RateLimiter>,
        panicking: bool,
    ) {
        let Some(keys) = self.active() else {
            return;
        };
        let allowed = if panicking {
            limiter
                .try_lock()
                .is_some_and(|mut l| l.allow(fingerprint, Instant::now()))
        } else {
            limiter.lock().allow(fingerprint, Instant::now())
        };
        if !allowed {
            return;
        }
        let Some(id) = self.distinct_id(panicking) else {
            return;
        };
        let body = event_body(&keys.key, event, &id, props, SystemTime::now());
        let body = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => return warn!("cannot serialise an error report: {e}"),
        };
        let url = endpoint(&keys.host);
        let transport = self.transport.clone();
        let event = event.to_string();
        let post = move |timeout| match transport.post(&url, &body, timeout) {
            Ok(()) => info!("sent {event} report"),
            Err(e) => warn!("could not send {event} report: {e}"),
        };
        if panicking {
            post(PANIC_WAIT);
        } else if let Err(e) = std::thread::Builder::new()
            .name("wtm-report".into())
            .spawn(move || post(SEND_TIMEOUT))
        {
            warn!("cannot start the report thread: {e}");
        }
    }

    /// The anonymous id, read from or written to `id_path` on first use.
    fn distinct_id(&self, panicking: bool) -> Option<String> {
        let mut slot = if panicking {
            self.id.try_lock()?
        } else {
            self.id.lock()
        };
        if !self.enabled.load(Ordering::Acquire) {
            return None;
        }
        if let Some(id) = slot.as_ref() {
            return Some(id.clone());
        }
        let stored = std::fs::read_to_string(&self.id_path)
            .ok()
            .and_then(|s| uuid::Uuid::parse_str(s.trim()).ok());
        let id = match stored {
            Some(id) => id.to_string(),
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                if let Some(dir) = self.id_path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                // Unwritten, it still holds for this run.
                if let Err(e) = std::fs::write(&self.id_path, &id) {
                    warn!("cannot write {}: {e}", self.id_path.display());
                }
                id
            }
        };
        *slot = Some(id.clone());
        Some(id)
    }
}

// MARK: Logged warnings

static LOG_SINK: OnceLock<Arc<Reporter>> = OnceLock::new();

/// Records logged before there was a reporter to take them: `App::new`
/// reads the config and the snapshot before it builds one. Only a few are
/// kept.
static EARLY: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

const MAX_EARLY: usize = 10;

/// Pass the warnings and errors this app logs to `reporter` from now on
/// (which still sends nothing unless reports are on), starting with any
/// logged before this call. The executable's logger calls [`forward`] with
/// every record.
pub fn forward_logs(reporter: Arc<Reporter>) {
    if LOG_SINK.set(reporter.clone()).is_err() {
        return;
    }
    // `forward` checks the sink again under this lock, so nothing can be
    // pushed after this takes the buffer.
    let early = std::mem::take(&mut *EARLY.lock());
    for (level, target, message) in early {
        reporter.log(level, &target, &message);
    }
}

/// Whether a log record goes to PostHog: a warning or an error from one of
/// this app's crates. Not other crates', whose messages this app does not
/// control, and not this module's own, which are about sending reports and
/// would report themselves in a loop.
pub fn reportable(level: log::Level, target: &str) -> bool {
    level <= log::Level::Warn && target.starts_with("wtm_") && !target.starts_with(module_path!())
}

/// Hand one log record to the reporter set by [`forward_logs`], if it is
/// [`reportable`].
pub fn forward(record: &log::Record) {
    if !reportable(record.level(), record.target()) {
        return;
    }
    if let Some(reporter) = LOG_SINK.get() {
        // Most people leave reports off; skip formatting the message then.
        if reporter.is_on() {
            reporter.log(record.level(), record.target(), &record.args().to_string());
        }
        return;
    }
    let mut early = EARLY.lock();
    match LOG_SINK.get() {
        // Set while this waited for the lock: the buffer is already drained.
        Some(reporter) => {
            drop(early);
            reporter.log(record.level(), record.target(), &record.args().to_string());
        }
        None if early.len() < MAX_EARLY => {
            early.push((
                record.level(),
                record.target().to_string(),
                record.args().to_string(),
            ));
        }
        None => {}
    }
}

/// Report panics through `reporter`, after whatever the hook already did
/// (printing the message), which runs first so the log has it even if the
/// send is slow.
pub fn install_panic_hook(reporter: Arc<Reporter>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic with a non-string payload".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current();
        reporter.panic(&message, location, thread.name());
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<(String, Value)>>);

    impl Transport for Recorder {
        fn post(&self, url: &str, body: &[u8], _: Duration) -> Result<(), String> {
            let body = serde_json::from_slice(body).map_err(|e| e.to_string())?;
            self.0.lock().push((url.to_string(), body));
            Ok(())
        }
    }

    impl Recorder {
        /// Waits for `n` posts: all but the panic's are sent from a thread.
        fn wait_for(&self, n: usize) -> Vec<(String, Value)> {
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.0.lock().len() < n && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.0.lock().clone()
        }
    }

    fn keys() -> Option<Keys> {
        keys_from(Some("phc_test"), Some("https://eu.i.posthog.com/"))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wtm-telemetry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn reporter(keys: Option<Keys>, dir: &std::path::Path) -> (Reporter, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let r = Reporter::new(
            keys,
            dir.join("analytics-id"),
            "/home/ada".into(),
            recorder.clone(),
        );
        (r, recorder)
    }

    #[test]
    fn keys_need_both_values_and_https() {
        assert_eq!(
            keys(),
            Some(Keys {
                key: "phc_test".into(),
                host: "https://eu.i.posthog.com".into()
            })
        );
        assert_eq!(keys_from(None, Some("https://x")), None);
        assert_eq!(keys_from(Some("k"), None), None);
        assert_eq!(keys_from(Some(" "), Some("https://x")), None);
        assert_eq!(keys_from(Some("k"), Some("http://x")), None);
        assert_eq!(keys_from(Some("k"), Some("https://")), None);
        assert_eq!(endpoint("https://x/"), "https://x/i/v0/e/");
    }

    #[test]
    fn a_debug_build_has_no_keys() {
        if cfg!(debug_assertions) {
            assert_eq!(compiled_in(), None);
        }
    }

    #[test]
    fn the_body_is_what_posthog_reads() {
        let at = UNIX_EPOCH + Duration::from_millis(1_792_229_400_123);
        let mut props = exception_properties("Push failed", "rejected", true);
        // No event can ask for a person profile.
        props.insert("$process_person_profile".into(), Value::Bool(true));
        let body = event_body("phc_k", "$exception", "id-1", props, at);
        assert_eq!(body["api_key"], "phc_k");
        assert_eq!(body["event"], "$exception");
        assert_eq!(body["distinct_id"], "id-1");
        assert_eq!(body["timestamp"], "2026-10-17T09:30:00.123Z");
        let p = &body["properties"];
        assert_eq!(p["$process_person_profile"], false);
        assert_eq!(p["$exception_level"], "error");
        assert_eq!(p["$exception_list"][0]["type"], "Push failed");
        assert_eq!(p["$exception_list"][0]["value"], "rejected");
        assert_eq!(p["$app_version"], VERSION);
        assert_eq!(p["arch"], std::env::consts::ARCH);
        assert!(p["$os"].is_string());
    }

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // A leap day, and the last second of a year.
        let leap = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(rfc3339(leap), "2024-02-29T00:00:00.000Z");
        let eve = UNIX_EPOCH + Duration::from_secs(1_735_689_599);
        assert_eq!(rfc3339(eve), "2024-12-31T23:59:59.000Z");
    }

    #[test]
    fn notices_are_grouped_by_what_failed() {
        assert_eq!(exception_type("Push failed: rejected"), "Push failed");
        assert_eq!(
            exception_type("Could not open editor: x"),
            "Could not open editor"
        );
        assert_eq!(exception_type("no colon here"), "Error");
        assert_eq!(exception_type("~/a/b: not found"), "Error");
        assert_eq!(exception_type(": empty"), "Error");
    }

    #[test]
    fn scrubbing_takes_out_the_home_folder_credentials_and_addresses() {
        assert_eq!(
            scrub("cannot read /home/ada/src/x: denied", "/home/ada"),
            "cannot read ~/src/x: denied"
        );
        assert_eq!(scrub("in /home/ada", "/home/ada/"), "in ~");
        assert_eq!(
            scrub("/home/adam/x", "/home/ada"),
            "/home/adam/x",
            "only a whole component"
        );
        assert_eq!(
            scrub(r"C:\Users\ada\src and C:/Users/ada/src", "C:/Users/ada"),
            r"~\src and ~/src"
        );
        assert_eq!(
            scrub("fatal: https://ada:ghp_secret@github.com/o/r.git", ""),
            "fatal: https://github.com/o/r.git"
        );
        assert_eq!(
            scrub("Author: Ada <ada.l@example.co.uk>", ""),
            "Author: Ada <<email>>"
        );
        assert_eq!(
            scrub("git@github.com:o/r.git: denied", ""),
            "git@github.com:o/r.git: denied"
        );
        let long = "é".repeat(MAX_MESSAGE + 10);
        assert_eq!(scrub(&long, "").chars().count(), MAX_MESSAGE + 1);
    }

    #[test]
    fn the_limiter_caps_an_hour_and_drops_repeats() {
        let mut l = RateLimiter::new(3, Duration::from_secs(3600));
        let t = Instant::now();
        assert!(l.allow("a", t));
        assert!(!l.allow("a", t), "the same event once per window");
        assert!(l.allow("b", t));
        assert!(l.allow("c", t));
        assert!(!l.allow("d", t), "three an hour");
        let later = t + Duration::from_secs(3601);
        assert!(l.allow("a", later));
        assert!(l.allow("d", later));
    }

    #[test]
    fn nothing_is_sent_while_off_or_without_keys() {
        let dir = scratch("off");
        let (r, rec) = reporter(keys(), &dir);
        r.error("Push failed: x");
        r.panic("boom", None, None);
        r.started();
        let (unkeyed, unkeyed_rec) = reporter(None, &dir);
        unkeyed.set_enabled(true);
        unkeyed.error("Push failed: x");
        unkeyed.started();
        std::thread::sleep(Duration::from_millis(100));
        assert!(rec.0.lock().is_empty());
        assert!(unkeyed_rec.0.lock().is_empty());
        assert!(!dir.join("analytics-id").exists(), "no id before opting in");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_opted_in_error_is_scrubbed_and_sent_with_a_lasting_id() {
        let dir = scratch("on");
        let (r, rec) = reporter(keys(), &dir);
        r.set_enabled(true);
        r.error("Pull failed: cannot lock /home/ada/w/x");
        r.error("Pull failed: cannot lock /home/ada/w/x");
        r.panic("boom", Some("src/app.rs:1:2".into()), Some("main"));
        let sent = rec.wait_for(2);
        std::thread::sleep(Duration::from_millis(50));
        let sent_after = rec.0.lock().len();
        assert_eq!(sent_after, 2, "the repeat is dropped");
        assert!(sent
            .iter()
            .all(|(url, _)| url == "https://eu.i.posthog.com/i/v0/e/"));
        let error = sent
            .iter()
            .map(|(_, b)| b)
            .find(|b| b["properties"]["$exception_list"][0]["type"] == "Pull failed")
            .expect("the error");
        assert_eq!(
            error["properties"]["$exception_list"][0]["value"],
            "Pull failed: cannot lock ~/w/x"
        );
        let panic = sent
            .iter()
            .map(|(_, b)| b)
            .find(|b| b["properties"]["$exception_list"][0]["type"] == "panic")
            .expect("the panic");
        assert_eq!(
            panic["properties"]["$exception_list"][0]["value"],
            "boom (at src/app.rs:1:2)"
        );
        assert_eq!(
            panic["properties"]["$exception_list"][0]["mechanism"]["handled"],
            false
        );
        let id = std::fs::read_to_string(dir.join("analytics-id")).unwrap();
        assert!(sent.iter().all(|(_, b)| b["distinct_id"] == id.as_str()));

        // A new run reads the same id; turning reports off forgets it.
        let (again, rec2) = reporter(keys(), &dir);
        again.set_enabled(true);
        again.started();
        assert_eq!(rec2.wait_for(1)[0].1["distinct_id"], id.as_str());
        again.set_enabled(false);
        assert!(!dir.join("analytics-id").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn turning_reports_on_while_running_says_so_and_launching_does_not() {
        let dir = scratch("toggle");
        let (r, rec) = reporter(keys(), &dir);
        // At launch: `started` is the event, not this.
        r.set_enabled(true);
        std::thread::sleep(Duration::from_millis(50));
        assert!(rec.0.lock().is_empty());
        r.set_enabled(false);
        r.set_enabled(true);
        let sent = rec.wait_for(1);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1["event"], "error reports turned on");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_this_apps_warnings_are_reported() {
        use log::Level;
        assert!(reportable(Level::Warn, "wtm_core::app"));
        assert!(reportable(Level::Error, "wtm_macos::updater"));
        assert!(!reportable(Level::Info, "wtm_core::app"));
        assert!(!reportable(Level::Warn, "notify::fsevent"));
        assert!(
            !reportable(Level::Warn, "wtm_core::telemetry"),
            "a failed send would report itself"
        );
    }

    #[test]
    fn a_logged_warning_is_sent_as_a_warning_named_by_its_module() {
        let dir = scratch("log");
        let (r, rec) = reporter(keys(), &dir);
        r.log(log::Level::Warn, "wtm_core::app", "off: not sent");
        r.set_enabled(true);
        r.log(log::Level::Warn, "wtm_core::app", "cannot save /home/ada/x");
        let sent = rec.wait_for(1);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(rec.0.lock().len(), 1);
        let p = &sent[0].1["properties"];
        assert_eq!(p["$exception_level"], "warning");
        assert_eq!(p["log_target"], "wtm_core::app");
        assert_eq!(p["$exception_list"][0]["type"], "wtm_core::app");
        assert_eq!(p["$exception_list"][0]["value"], "cannot save ~/x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn warnings_cannot_use_up_the_errors_allowance() {
        let dir = scratch("budget");
        let (r, rec) = reporter(keys(), &dir);
        r.set_enabled(true);
        for i in 0..MAX_LOGGED_PER_HOUR + 5 {
            r.log(log::Level::Warn, "wtm_core::fetcher", &format!("fetch {i}"));
        }
        r.error("Push failed: rejected");
        let sent = rec.wait_for(MAX_LOGGED_PER_HOUR + 1);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(rec.0.lock().len(), MAX_LOGGED_PER_HOUR + 1);
        assert!(sent
            .iter()
            .any(|(_, b)| b["properties"]["$exception_list"][0]["type"] == "Push failed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The only test that touches the global sink, which can be set once.
    #[test]
    fn records_logged_before_the_reporter_exists_are_sent_once_it_does() {
        let record = |level, target: &str, text: &str| {
            let args = format_args!("{text}");
            forward(
                &log::Record::builder()
                    .level(level)
                    .target(target)
                    .args(args)
                    .build(),
            );
        };
        for i in 0..MAX_EARLY + 2 {
            record(log::Level::Error, "wtm_core::app", &format!("early {i}"));
        }
        record(log::Level::Info, "wtm_core::app", "not a warning");
        let dir = scratch("sink");
        let (r, rec) = reporter(keys(), &dir);
        r.set_enabled(true);
        forward_logs(Arc::new(r));
        record(log::Level::Error, "wtm_core::app", "late");
        record(log::Level::Warn, "notify", "not ours");
        let sent = rec.wait_for(MAX_EARLY + 1);
        std::thread::sleep(Duration::from_millis(50));
        let values: Vec<String> = rec
            .0
            .lock()
            .iter()
            .map(|(_, b)| b["properties"]["$exception_list"][0]["value"].to_string())
            .collect();
        assert_eq!(values.len(), MAX_EARLY + 1, "{values:?}");
        assert!(values.iter().any(|v| v.contains("early 0")));
        assert!(!values
            .iter()
            .any(|v| v.contains(&format!("early {MAX_EARLY}"))));
        assert!(sent
            .iter()
            .any(|(_, b)| b["properties"]["$exception_level"] == "error"));
        assert!(values.iter().any(|v| v.contains("late")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_logged_error_does_not_wait_behind_warnings() {
        let dir = scratch("errors");
        let (r, rec) = reporter(keys(), &dir);
        r.set_enabled(true);
        for i in 0..MAX_LOGGED_PER_HOUR {
            r.log(log::Level::Warn, "wtm_core::fetcher", &format!("w {i}"));
        }
        r.log(log::Level::Error, "wtm_macos::updater", "lost the app");
        rec.wait_for(MAX_LOGGED_PER_HOUR + 1);
        std::thread::sleep(Duration::from_millis(50));
        assert!(rec
            .0
            .lock()
            .iter()
            .any(|(_, b)| b["properties"]["$exception_list"][0]["value"] == "lost the app"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn turning_reports_on_again_says_so_again() {
        let dir = scratch("again");
        let (r, rec) = reporter(keys(), &dir);
        r.set_enabled(false);
        r.set_enabled(true);
        r.set_enabled(false);
        r.set_enabled(true);
        assert_eq!(rec.wait_for(2).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_left_from_an_earlier_run_goes_when_reports_are_off() {
        let dir = scratch("stale");
        std::fs::create_dir_all(&dir).unwrap();
        let id_path = dir.join("analytics-id");
        std::fs::write(&id_path, uuid::Uuid::new_v4().to_string()).unwrap();
        let (r, _) = reporter(keys(), &dir);
        r.set_enabled(false);
        assert!(!id_path.exists());
        // Off, an id is never made, even by a report already under way.
        assert_eq!(r.distinct_id(false), None);
        assert!(!id_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn curl_reads_the_body_from_stdin() {
        let args = curl_args("https://x/i/v0/e/", Duration::from_millis(10));
        assert_eq!(args.last().map(String::as_str), Some("https://x/i/v0/e/"));
        let at = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(at("--data-binary"), "@-");
        assert_eq!(at("--max-time"), "1");
        assert!(args.iter().all(|a| !a.contains("phc_")));
    }
}
