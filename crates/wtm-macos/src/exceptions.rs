//! Remembers the last Objective-C exception thrown, for the panic report.
//!
//! When AppKit raises an exception (a splice naming rows the outline does not
//! hold, say) and it unwinds into a Rust frame, Rust aborts with "panic in a
//! function that cannot unwind" and the exception's name and reason are
//! lost. The runtime's exception preprocessor sees every exception as it is
//! thrown, before any unwinding, so it can note them for the panic hook.
//! AppKit throws and catches some exceptions of its own, which is why only a
//! recent one is reported.

use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use objc2::runtime::AnyObject;
use objc2_foundation::NSException;

type Preprocessor = unsafe extern "C" fn(*mut AnyObject) -> *mut AnyObject;

/// The abort follows the throw at once, so anything older is unrelated.
const RECENT: Duration = Duration::from_secs(2);

static LAST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static PREVIOUS: OnceLock<Preprocessor> = OnceLock::new();

/// Start noting exceptions. Only the first call does anything: a second
/// would chain `note` to itself.
pub fn watch() {
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        // SAFETY: `note` has the preprocessor's signature and hands its
        // argument on, as the runtime requires.
        let previous = unsafe { objc2::ffi::objc_setExceptionPreprocessor(note) };
        let _ = PREVIOUS.set(previous);
    });
}

/// The exception thrown in the last two seconds, as "name: reason".
pub fn recent() -> Option<String> {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let (at, what) = last.as_ref()?;
    (at.elapsed() < RECENT).then(|| what.clone())
}

unsafe extern "C" fn note(exception: *mut AnyObject) -> *mut AnyObject {
    // SAFETY: the runtime passes the object being thrown, or null.
    if let Some(object) = unsafe { exception.as_ref() } {
        let what = match object.downcast_ref::<NSException>() {
            Some(e) => match e.reason() {
                Some(reason) => format!("{}: {reason}", e.name()),
                None => e.name().to_string(),
            },
            None => format!("a thrown {}", object.class().name().to_string_lossy()),
        };
        // Formatted before locking, so the lock is only ever held for a
        // store and `recent` can wait for it.
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), what));
    }
    match PREVIOUS.get() {
        // SAFETY: it was the runtime's preprocessor before ours.
        Some(previous) => unsafe { previous(exception) },
        // Only between installing `note` and storing what it replaced.
        None => exception,
    }
}
