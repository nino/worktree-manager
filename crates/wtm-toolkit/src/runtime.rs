//! The loop every backend shares: messages in, `update`, one `view`, one
//! `render`, then the effects.
//!
//! A handler in the view only queues its message; the queue is drained on
//! the next turn of the main loop, through the backend's `post`. So a
//! handler a native callback fires (AppKit's will-expand notification, a
//! drag's drop, a field's text being written during a render) never renders
//! inside that callback. That one rule keeps every backend free of
//! re-entrancy: the program is never called while it is already running,
//! and a backend is never asked to render while it is rendering, or while
//! the toolkit is in the middle of its own bookkeeping.
//!
//! [`Runtime::flush`] drains at once, for the few places that must see the
//! result before returning: quitting (the window state is written before
//! the process exits) and menu validation (⌘N straight after an arrow key
//! must see the new selection).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use crate::view::{Effect, Handler, HostEvent, View};

/// The app's UI. Implemented once, in `wtm-ui`.
pub trait Program: 'static {
    type Msg: Clone + Send + 'static;

    fn update(&mut self, msg: Self::Msg, cx: &mut Cx<Self::Msg>);

    fn host(&mut self, event: HostEvent, cx: &mut Cx<Self::Msg>);

    fn view(&self, cx: &ViewCx<Self::Msg>) -> View;
}

/// What a toolkit implements to show a [`View`].
pub trait Backend {
    /// Bring the native widgets in line with `view`. Called once per batch
    /// of messages, with the whole view.
    fn render(&self, view: &View);

    /// Carry out a one-off command, after the render of the same batch.
    fn perform(&self, effect: Effect);
}

/// The entry point a toolkit crate offers: it owns the native event loop.
pub trait Toolkit {
    fn run<P: Program>(self, program: P) -> !;
}

/// Runs a closure on the main thread, from any thread. Supplied by the
/// backend (a main-queue dispatch, a GLib idle source, a posted window
/// message).
pub type Poster = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// Makes handlers that send messages. Given to [`Program::view`].
pub struct ViewCx<M> {
    send: Rc<dyn Fn(M)>,
}

impl<M: Clone + 'static> ViewCx<M> {
    /// A handler that sends `msg`.
    pub fn on(&self, msg: M) -> Handler {
        let send = self.send.clone();
        Handler::new(move |()| send(msg.clone()))
    }

    /// A handler that sends what `f` makes of its argument.
    pub fn map<A: 'static>(&self, f: impl Fn(A) -> M + 'static) -> Handler<A> {
        let send = self.send.clone();
        Handler::new(move |a| send(f(a)))
    }

    /// A handler that sends `msg` only when `f` says so.
    pub fn filter_map<A: 'static>(&self, f: impl Fn(A) -> Option<M> + 'static) -> Handler<A> {
        let send = self.send.clone();
        Handler::new(move |a| {
            if let Some(m) = f(a) {
                send(m)
            }
        })
    }
}

/// Sends messages to the program from any thread. Cheap to clone.
pub struct Proxy<M> {
    post: Poster,
    _msg: PhantomData<fn(M)>,
}

impl<M> Clone for Proxy<M> {
    fn clone(&self) -> Self {
        Proxy {
            post: self.post.clone(),
            _msg: PhantomData,
        }
    }
}

impl<M: Send + 'static> Proxy<M> {
    /// Queue `msg` for the program, on the main thread. Dropped silently if
    /// the runtime has gone (the app is exiting).
    pub fn send(&self, msg: M) {
        (self.post)(Box::new(move || {
            CURRENT.with(|c| {
                let driver = c.borrow().clone();
                if let Some(driver) = driver {
                    driver.send_any(Box::new(msg));
                }
            })
        }));
    }
}

/// What `update` and `host` can do besides changing the program's state.
pub struct Cx<M> {
    view: ViewCx<M>,
    proxy: Proxy<M>,
    effects: Vec<Effect>,
    render: bool,
}

impl<M: Clone + Send + 'static> Cx<M> {
    /// Carry out `effect` after this batch's render.
    pub fn effect(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    /// A handle for sending messages from other threads.
    pub fn proxy(&self) -> Proxy<M> {
        self.proxy.clone()
    }

    /// A handler that sends `msg` (for effects that call back).
    pub fn on(&self, msg: M) -> Handler {
        self.view.on(msg)
    }

    pub fn map<A: 'static>(&self, f: impl Fn(A) -> M + 'static) -> Handler<A> {
        self.view.map(f)
    }

    /// Nothing on screen changes because of this message (the list was
    /// scrolled, the window moved). If every message in a batch says so, the
    /// batch is not rendered.
    pub fn unchanged(&mut self) {
        self.render = false;
    }
}

/// The type-erased side of a runtime, reachable from posted closures and
/// backends through [`host_event`].
trait Driver {
    fn send_any(&self, msg: Box<dyn Any>);
    fn host(&self, event: HostEvent);
    fn drain(&self);
}

thread_local! {
    /// The runtime of the UI thread.
    static CURRENT: RefCell<Option<Rc<dyn Driver>>> = const { RefCell::new(None) };
}

/// Tell the program something no handler covers (see [`HostEvent`]). Called
/// by backends on the main thread; queued like any message.
pub fn host_event(event: HostEvent) {
    let driver = CURRENT.with(|c| c.borrow().clone());
    if let Some(driver) = driver {
        driver.host(event);
    }
}

enum Item<M> {
    Msg(M),
    Host(HostEvent),
}

/// Owns the program and the queue; see the module docs.
pub struct Runtime<P: Program> {
    program: RefCell<P>,
    queue: RefCell<VecDeque<Item<P::Msg>>>,
    busy: Cell<bool>,
    /// A drain has been posted and not yet run.
    scheduled: Cell<bool>,
    backend: RefCell<Option<Rc<dyn Backend>>>,
    post: Poster,
    me: Weak<Self>,
}

impl<P: Program> Runtime<P> {
    /// Install `program` as this thread's UI, with the backend `make`
    /// builds. Does not render yet: call [`Runtime::start`] once the
    /// toolkit is ready to show windows.
    pub fn new(program: P, post: Poster, make: impl FnOnce() -> Rc<dyn Backend>) -> Rc<Self> {
        let rt = Rc::new_cyclic(|me| Runtime {
            program: RefCell::new(program),
            queue: RefCell::new(VecDeque::new()),
            busy: Cell::new(false),
            scheduled: Cell::new(false),
            backend: RefCell::new(None),
            post,
            me: me.clone(),
        });
        CURRENT.with(|c| *c.borrow_mut() = Some(rt.clone() as Rc<dyn Driver>));
        *rt.backend.borrow_mut() = Some(make());
        rt
    }

    /// Deliver [`HostEvent::Started`], which renders the first view.
    pub fn start(&self, screens: Vec<crate::view::Frame>) {
        self.push(Item::Host(HostEvent::Started { screens }));
    }

    /// Run `f` with the program, between batches. For tests.
    pub fn with_program<R>(&self, f: impl FnOnce(&P) -> R) -> R {
        f(&self.program.borrow())
    }

    pub fn send(&self, msg: P::Msg) {
        self.push(Item::Msg(msg));
    }

    fn push(&self, item: Item<P::Msg>) {
        self.queue.borrow_mut().push_back(item);
        if self.scheduled.replace(true) {
            return;
        }
        (self.post)(Box::new(|| {
            let driver = CURRENT.with(|c| c.borrow().clone());
            if let Some(driver) = driver {
                driver.drain();
            }
        }));
    }

    /// Drain the queue now, unless it is already being drained further up
    /// the stack. See the module docs for when.
    pub fn flush(&self) {
        self.scheduled.set(false);
        self.pump();
    }

    fn view_cx(&self) -> ViewCx<P::Msg> {
        let me = self.me.clone();
        ViewCx {
            send: Rc::new(move |m| {
                if let Some(rt) = me.upgrade() {
                    rt.send(m);
                }
            }),
        }
    }

    fn pump(&self) {
        if self.busy.replace(true) {
            return;
        }
        let mut rounds = 0;
        loop {
            rounds += 1;
            // A program that answers every render with another message
            // would spin here forever; that is a bug in the program. A
            // release build hands the main loop back instead: whatever is
            // still queued has a drain posted for it already.
            if rounds > 1000 {
                debug_assert!(false, "the program keeps sending itself messages");
                log::error!("the program keeps sending itself messages");
                break;
            }
            let items: Vec<_> = self.queue.borrow_mut().drain(..).collect();
            if items.is_empty() {
                break;
            }
            let mut cx = Cx {
                view: self.view_cx(),
                proxy: Proxy {
                    post: self.post.clone(),
                    _msg: PhantomData,
                },
                effects: Vec::new(),
                render: false,
            };
            let mut render = false;
            for item in items {
                cx.render = true;
                {
                    let mut program = self.program.borrow_mut();
                    match item {
                        Item::Msg(m) => program.update(m, &mut cx),
                        Item::Host(e) => program.host(e, &mut cx),
                    }
                }
                render |= cx.render;
            }
            let backend = self.backend.borrow().clone();
            let Some(backend) = backend else { continue };
            if render {
                let view = self.program.borrow().view(&cx.view);
                backend.render(&view);
            }
            for effect in cx.effects {
                backend.perform(effect);
            }
        }
        self.busy.set(false);
    }
}

impl<P: Program> Driver for Runtime<P> {
    fn send_any(&self, msg: Box<dyn Any>) {
        match msg.downcast::<P::Msg>() {
            Ok(m) => self.send(*m),
            Err(_) => log::error!("a message of the wrong type reached the runtime"),
        }
    }

    fn host(&self, event: HostEvent) {
        self.push(Item::Host(event));
    }

    fn drain(&self) {
        self.flush();
    }
}

/// Drain this thread's runtime now (see [`Runtime::flush`]). For backends,
/// which hold the runtime only type-erased.
pub fn flush() {
    let driver = CURRENT.with(|c| c.borrow().clone());
    if let Some(driver) = driver {
        driver.drain();
    }
}

/// Forget this thread's runtime, so a test can install another.
pub fn uninstall() {
    CURRENT.with(|c| c.borrow_mut().take());
}
