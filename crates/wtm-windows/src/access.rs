//! What assistive technology reads of the list: the cards and rows as the
//! items of an outline, with names, states and places on screen.
//!
//! The cards are painted, not controls, so the list's frame answers
//! `WM_GETOBJECT` with an MSAA object of its own whose simple children
//! (child ids from 1) are the rows on screen, in order. The selection is
//! raised as focus and selection events on those ids. Everything else (the
//! frame's own place, its parent, its role as a window) is the standard
//! object's. UI Automation reaches this through its MSAA proxy; a native
//! UIA provider would add patterns (expand/collapse, invoke) and is not
//! attempted.
//!
//! The object reads a snapshot the list publishes after each layout, never
//! the backend's state: a screen reader's calls can arrive while that is
//! borrowed.

use std::cell::RefCell;

use windows::core::{implement, Interface, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::{
    DISP_E_MEMBERNOTFOUND, E_INVALIDARG, E_NOTIMPL, HWND, LPARAM, LRESULT, POINT, RECT, S_FALSE,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Com::{
    IDispatch, IDispatch_Impl, ITypeInfo, DISPATCH_FLAGS, DISPPARAMS, EXCEPINFO,
};
use windows::Win32::System::Variant::{VARIANT, VT_EMPTY, VT_I4};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

/// One item of the outline.
pub struct Item {
    pub name: String,
    /// In the list's content window, which scrolls inside the frame.
    pub rect: RECT,
    /// A card's header, which opens and closes; else a row in a card.
    pub header: bool,
    pub expanded: bool,
}

#[derive(Default)]
struct Snapshot {
    frame: HWND,
    content: HWND,
    items: Vec<Item>,
    selected: Option<usize>,
    /// What the last events announced: the selected item and its name.
    announced: Option<(usize, String)>,
}

thread_local! {
    static SNAP: RefCell<Snapshot> = RefCell::new(Snapshot::default());
}

const STATE_FOCUSABLE: u32 = 0x0010_0000;
const STATE_OFFSCREEN: u32 = 0x0001_0000;

/// The list as it now stands. Raises focus and selection events when the
/// selected item changed and the list has the keyboard.
pub fn publish(frame: HWND, content: HWND, items: Vec<Item>, selected: Option<usize>) {
    SNAP.with(|s| {
        let mut s = s.borrow_mut();
        s.frame = frame;
        s.content = content;
        s.items = items;
        s.selected = selected;
    });
    announce(false);
}

/// Tell assistive technology which item is selected, if the list has the
/// keyboard. `again`: even if it was already told (the list just took the
/// keyboard).
pub fn announce(again: bool) {
    let event = SNAP.with(|s| {
        let mut s = s.borrow_mut();
        let now = s.selected.map(|i| (i, s.items[i].name.clone()));
        if now.is_none() || (!again && now == s.announced) {
            return None;
        }
        if unsafe { GetFocus() } != s.frame {
            return None;
        }
        s.announced = now.clone();
        now.map(|(i, _)| (s.frame, i as i32 + 1))
    });
    // Outside the borrow: a hook in this process may ask for the object
    // from inside these calls.
    if let Some((frame, id)) = event {
        unsafe {
            NotifyWinEvent(EVENT_OBJECT_SELECTION, frame, OBJID_CLIENT.0, id);
            NotifyWinEvent(EVENT_OBJECT_FOCUS, frame, OBJID_CLIENT.0, id);
        }
    }
}

/// The frame's answer to `WM_GETOBJECT`; `None`: the default's.
pub fn get_object(frame: HWND, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    if l.0 as i32 != OBJID_CLIENT.0 {
        return None;
    }
    let mut std: Option<IAccessible> = None;
    unsafe {
        CreateStdAccessibleObject(
            frame,
            OBJID_CLIENT.0,
            &IAccessible::IID,
            &mut std as *mut _ as *mut _,
        )
        .ok()?;
    }
    let acc: IAccessible = ListAccessible { frame, std: std? }.into();
    Some(unsafe { LresultFromObject(&IAccessible::IID, w, &acc) })
}

// MARK: Variants

fn var_i4(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        let inner = &mut *var.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = v;
    }
    var
}

fn var_empty() -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        (*var.Anonymous.Anonymous).vt = VT_EMPTY;
    }
    var
}

/// The child id a variant names; 0 is the list itself.
fn child_id(v: &VARIANT) -> windows::core::Result<i32> {
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        if inner.vt == VT_I4 {
            Ok(inner.Anonymous.lVal)
        } else {
            Err(E_INVALIDARG.into())
        }
    }
}

/// The snapshot's item for child id `id` (from 1).
fn with_item<R>(id: i32, f: impl FnOnce(&Item, bool) -> R) -> windows::core::Result<R> {
    SNAP.with(|s| {
        let s = s.borrow();
        let i = usize::try_from(id - 1).map_err(|_| E_INVALIDARG)?;
        let item = s.items.get(i).ok_or(E_INVALIDARG)?;
        Ok(f(item, s.selected == Some(i)))
    })
}

/// An item's rectangle on screen, and whether any of it shows in the frame.
fn screen_rect(item: &Item) -> (RECT, bool) {
    SNAP.with(|s| {
        let s = s.borrow();
        let mut tl = POINT {
            x: item.rect.left,
            y: item.rect.top,
        };
        let mut origin = POINT::default();
        let mut frame = RECT::default();
        unsafe {
            let _ = ClientToScreen(s.content, &mut tl);
            let _ = ClientToScreen(s.frame, &mut origin);
            let _ = GetClientRect(s.frame, &mut frame);
        }
        let r = RECT {
            left: tl.x,
            top: tl.y,
            right: tl.x + item.rect.right - item.rect.left,
            bottom: tl.y + item.rect.bottom - item.rect.top,
        };
        let shown = r.bottom > origin.y && r.top < origin.y + frame.bottom;
        (r, shown)
    })
}

fn none<T>() -> windows::core::Result<T> {
    Err(S_FALSE.into())
}

// MARK: The object

#[implement(IAccessible)]
struct ListAccessible {
    frame: HWND,
    /// What Windows itself would answer for the frame.
    std: IAccessible,
}

impl IDispatch_Impl for ListAccessible_Impl {
    fn GetTypeInfoCount(&self) -> windows::core::Result<u32> {
        Ok(0)
    }

    fn GetTypeInfo(&self, _: u32, _: u32) -> windows::core::Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }

    fn GetIDsOfNames(
        &self,
        _: *const GUID,
        _: *const PCWSTR,
        _: u32,
        _: u32,
        _: *mut i32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Invoke(
        &self,
        _: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        _: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> windows::core::Result<()> {
        Err(DISP_E_MEMBERNOTFOUND.into())
    }
}

impl IAccessible_Impl for ListAccessible_Impl {
    fn accParent(&self) -> windows::core::Result<IDispatch> {
        unsafe { self.std.accParent() }
    }

    fn accChildCount(&self) -> windows::core::Result<i32> {
        Ok(SNAP.with(|s| s.borrow().items.len() as i32))
    }

    fn get_accChild(&self, _: &VARIANT) -> windows::core::Result<IDispatch> {
        // The children are simple elements, not objects of their own.
        none()
    }

    fn get_accName(&self, v: &VARIANT) -> windows::core::Result<BSTR> {
        match child_id(v)? {
            0 => Ok(BSTR::from("Repositories")),
            id => with_item(id, |item, _| BSTR::from(item.name.as_str())),
        }
    }

    fn get_accValue(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        none()
    }

    fn get_accDescription(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        none()
    }

    fn get_accRole(&self, v: &VARIANT) -> windows::core::Result<VARIANT> {
        Ok(var_i4(match child_id(v)? {
            0 => ROLE_SYSTEM_OUTLINE,
            _ => ROLE_SYSTEM_OUTLINEITEM,
        } as i32))
    }

    fn get_accState(&self, v: &VARIANT) -> windows::core::Result<VARIANT> {
        let id = child_id(v)?;
        if id == 0 {
            return unsafe { self.std.get_accState(v) };
        }
        let focused = unsafe { GetFocus() } == self.frame;
        let (state, rect_item) = with_item(id, |item, selected| {
            let mut state = STATE_SYSTEM_SELECTABLE | STATE_FOCUSABLE;
            if selected {
                state |= STATE_SYSTEM_SELECTED;
                if focused {
                    state |= STATE_SYSTEM_FOCUSED;
                }
            }
            if item.header {
                state |= if item.expanded {
                    STATE_SYSTEM_EXPANDED
                } else {
                    STATE_SYSTEM_COLLAPSED
                };
            }
            (state, screen_rect(item).1)
        })?;
        let state = if rect_item {
            state
        } else {
            state | STATE_OFFSCREEN
        };
        Ok(var_i4(state as i32))
    }

    fn get_accHelp(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        none()
    }

    fn get_accHelpTopic(&self, _: *mut BSTR, _: &VARIANT) -> windows::core::Result<i32> {
        none()
    }

    fn get_accKeyboardShortcut(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        none()
    }

    fn accFocus(&self) -> windows::core::Result<VARIANT> {
        if unsafe { GetFocus() } != self.frame {
            return unsafe { self.std.accFocus() };
        }
        let selected = SNAP.with(|s| s.borrow().selected);
        Ok(match selected {
            Some(i) => var_i4(i as i32 + 1),
            None => var_i4(0),
        })
    }

    fn accSelection(&self) -> windows::core::Result<VARIANT> {
        let selected = SNAP.with(|s| s.borrow().selected);
        Ok(match selected {
            Some(i) => var_i4(i as i32 + 1),
            None => var_empty(),
        })
    }

    fn get_accDefaultAction(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        none()
    }

    fn accSelect(&self, _: i32, _: &VARIANT) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn accLocation(
        &self,
        x: *mut i32,
        y: *mut i32,
        w: *mut i32,
        h: *mut i32,
        v: &VARIANT,
    ) -> windows::core::Result<()> {
        let id = child_id(v)?;
        if id == 0 {
            return unsafe { self.std.accLocation(x, y, w, h, v) };
        }
        let r = with_item(id, |item, _| screen_rect(item).0)?;
        if x.is_null() || y.is_null() || w.is_null() || h.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *x = r.left;
            *y = r.top;
            *w = r.right - r.left;
            *h = r.bottom - r.top;
        }
        Ok(())
    }

    fn accNavigate(&self, dir: i32, start: &VARIANT) -> windows::core::Result<VARIANT> {
        let id = child_id(start)?;
        let n = SNAP.with(|s| s.borrow().items.len()) as i32;
        let dir = dir as u32;
        let to = match (id, dir) {
            (0, NAVDIR_FIRSTCHILD) => 1,
            (0, NAVDIR_LASTCHILD) => n,
            (0, _) => return unsafe { self.std.accNavigate(dir as i32, start) },
            (_, NAVDIR_NEXT | NAVDIR_DOWN) => id + 1,
            (_, NAVDIR_PREVIOUS | NAVDIR_UP) => id - 1,
            _ => 0,
        };
        if to < 1 || to > n {
            return none();
        }
        Ok(var_i4(to))
    }

    fn accHitTest(&self, x: i32, y: i32) -> windows::core::Result<VARIANT> {
        let hit = SNAP.with(|s| {
            let s = s.borrow();
            let mut p = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(s.content, &mut p);
            }
            s.items.iter().position(|i| {
                p.x >= i.rect.left && p.x < i.rect.right && p.y >= i.rect.top && p.y < i.rect.bottom
            })
        });
        match hit {
            // A row's buttons are windows of their own; the standard object
            // finds them.
            Some(i) if !over_child_window(self.frame, x, y) => Ok(var_i4(i as i32 + 1)),
            _ => unsafe { self.std.accHitTest(x, y) },
        }
    }

    fn accDoDefaultAction(&self, _: &VARIANT) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn put_accName(&self, _: &VARIANT, _: &BSTR) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn put_accValue(&self, _: &VARIANT, _: &BSTR) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
}

/// A visible control of a row is under the screen point.
fn over_child_window(frame: HWND, x: i32, y: i32) -> bool {
    let under = unsafe { WindowFromPoint(POINT { x, y }) };
    let content = SNAP.with(|s| s.borrow().content);
    !under.is_invalid() && under != frame && under != content
}
