//! A backend with no widgets: it keeps the last [`View`] and the effects it
//! was asked for, and runs posted closures and timers when told to. Tests
//! drive a real program through it — press a button by calling its handler,
//! wait for background work to post its results, look at what the view
//! shows.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::{uninstall, Backend, Poster, Program, Runtime};
use crate::view::{Effect, Frame, Handler, View};

type Posted = Arc<Mutex<VecDeque<Box<dyn FnOnce() + Send>>>>;

#[derive(Default)]
struct State {
    view: Option<View>,
    renders: usize,
    effects: Vec<Effect>,
    /// `After` effects not yet fired, with when they are due.
    timers: Vec<(Instant, Handler)>,
}

/// The backend half of a [`Harness`].
pub struct Headless {
    state: RefCell<State>,
}

impl Backend for Headless {
    fn render(&self, view: &View) {
        let mut s = self.state.borrow_mut();
        s.view = Some(view.clone());
        s.renders += 1;
    }

    fn perform(&self, effect: Effect) {
        let mut s = self.state.borrow_mut();
        if let Effect::After(delay, then) = &effect {
            s.timers.push((Instant::now() + *delay, then.clone()));
        }
        s.effects.push(effect);
    }
}

/// A program running on the headless backend, on the calling thread.
pub struct Harness<P: Program> {
    runtime: Rc<Runtime<P>>,
    backend: Rc<Headless>,
    posted: Posted,
}

impl<P: Program> Harness<P> {
    /// Start `program` as if on a single screen of `screen`'s size.
    pub fn new(program: P, screen: Frame) -> Self {
        let posted: Posted = Arc::new(Mutex::new(VecDeque::new()));
        let queue = posted.clone();
        let post: Poster = Arc::new(move |f| queue.lock().unwrap().push_back(f));
        let backend = Rc::new(Headless {
            state: RefCell::new(State::default()),
        });
        let b = backend.clone();
        let runtime = Runtime::new(program, post, move || b as Rc<dyn Backend>);
        runtime.start(vec![screen]);
        let h = Harness {
            runtime,
            backend,
            posted,
        };
        h.pump();
        h
    }

    /// The last view rendered.
    pub fn view(&self) -> View {
        self.backend
            .state
            .borrow()
            .view
            .clone()
            .expect("nothing rendered yet")
    }

    /// How many times the backend has been asked to render.
    pub fn renders(&self) -> usize {
        self.backend.state.borrow().renders
    }

    /// Every effect performed so far, oldest first.
    pub fn effects(&self) -> Vec<Effect> {
        self.backend.state.borrow().effects.clone()
    }

    /// Forget the effects performed so far.
    pub fn clear_effects(&self) {
        self.backend.state.borrow_mut().effects.clear();
    }

    /// Send `msg` and run the turn that drains it.
    pub fn send(&self, msg: P::Msg) {
        self.runtime.send(msg);
        self.pump();
    }

    pub fn host(&self, event: crate::view::HostEvent) {
        crate::runtime::host_event(event);
        self.pump();
    }

    pub fn with_program<R>(&self, f: impl FnOnce(&P) -> R) -> R {
        self.runtime.with_program(f)
    }

    /// Run whatever other threads have posted. Returns how many ran.
    pub fn pump(&self) -> usize {
        let mut ran = 0;
        loop {
            let next = self.posted.lock().unwrap().pop_front();
            let Some(f) = next else { break };
            f();
            ran += 1;
        }
        ran
    }

    /// Fire every timer that is due by `now + ahead`, as if that much time
    /// had passed. Returns how many fired.
    pub fn advance(&self, ahead: Duration) -> usize {
        let due = Instant::now() + ahead;
        let fired: Vec<Handler> = {
            let mut s = self.backend.state.borrow_mut();
            let (fire, keep): (Vec<_>, Vec<_>) = s.timers.drain(..).partition(|(at, _)| *at <= due);
            s.timers = keep;
            fire.into_iter().map(|(_, h)| h).collect()
        };
        let n = fired.len();
        for h in fired {
            h.call(());
        }
        self.pump();
        n
    }

    /// Pump posted work until `done` holds for the view, or panic after
    /// `timeout`. For waiting on the core's background work.
    pub fn wait_for(&self, timeout: Duration, what: &str, done: impl Fn(&View) -> bool) -> View {
        let start = Instant::now();
        loop {
            self.pump();
            let view = self.backend.state.borrow().view.clone();
            if let Some(v) = view {
                if done(&v) {
                    return v;
                }
            }
            if start.elapsed() > timeout {
                panic!("timed out waiting for {what}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Answer the most recent folder picker with `paths`.
    pub fn choose_folders(&self, paths: Vec<PathBuf>) {
        let picker = self
            .backend
            .state
            .borrow()
            .effects
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::PickFolders(p) => Some(p.on_done.clone()),
                _ => None,
            })
            .expect("no folder picker was opened");
        picker.call(paths);
        self.pump();
    }

    /// Call `handler` as the backend would, and run the turn it queues.
    pub fn fire<A>(&self, handler: &Handler<A>, arg: A) {
        handler.call(arg);
        self.pump();
    }
}

impl<P: Program> Drop for Harness<P> {
    fn drop(&mut self) {
        uninstall();
    }
}
