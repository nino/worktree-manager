//! The view's popover (the branch picker): an `NSPopover` under a row's
//! branch button with a filter field above a keyboard-navigable list. The
//! filtering, the ranking and the selection are the shared UI's; this shows
//! them and reports keys and clicks: typing, ↑/↓, Return, Escape, and a
//! click outside.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use dispatch2::{DispatchQueue, MainThreadBound};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::Message;
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplicationDidResignActiveNotification, NSColor, NSControl, NSControlTextEditingDelegate,
    NSEvent, NSEventMask, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSLayoutConstraint, NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSScrollView,
    NSTableCellView, NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField,
    NSTextFieldDelegate, NSTextView, NSUserInterfaceItemIdentification, NSView, NSViewController,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSIndexSet, NSInteger, NSMutableAttributedString,
    NSMutableIndexSet, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint,
    NSRect, NSRectEdge, NSSize, NSString,
};
use wtm_toolkit::{FilterList, Popover};

use crate::branchlabel::rich_label;
use crate::util::{label, ns, SEMIBOLD};

const WIDTH: f64 = 300.0;
const LIST_HEIGHT: f64 = 208.0;
const PAD: f64 = 8.0;
const ROW_HEIGHT: f64 = 22.0;

thread_local! {
    /// The open picker, if any: the popover keeps no strong reference to
    /// its delegate and data source.
    static CURRENT: RefCell<Option<Retained<BranchPicker>>> = const { RefCell::new(None) };
    /// Pickers the view closed, kept until their popover has finished
    /// closing and said so: until then it still calls their delegate and
    /// data source methods.
    static CLOSING: RefCell<Vec<Retained<BranchPicker>>> = const { RefCell::new(Vec::new()) };
}

pub struct BranchPickerIvars {
    /// The view's id for this popover.
    id: u64,
    popover: Retained<NSPopover>,
    field: Retained<NSTextField>,
    table: Retained<NSTableView>,
    /// As last rendered.
    list: RefCell<FilterList>,
    /// The row the text was last drawn selected for.
    selected: Cell<NSInteger>,
    /// The button the popover hangs off, and what watches for the clicks
    /// that dismiss it.
    anchor: Retained<NSView>,
    monitor: RefCell<Option<Retained<AnyObject>>>,
    /// The view took the popover away: its closing is not the user's.
    closed_by_view: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMBranchPicker"]
    #[ivars = BranchPickerIvars]
    pub struct BranchPicker;

    unsafe impl NSObjectProtocol for BranchPicker {}

    impl BranchPicker {
        /// Leaving the app takes the picker with it, as a transient popover
        /// would. (The app's own window resigning key is not the same thing:
        /// the popover itself takes key while the filter field is typed in.)
        #[unsafe(method(appResignedActive:))]
        fn app_resigned_active(&self, _n: &NSNotification) {
            self.stop_watching();
            unsafe { self.ivars().popover.performClose(None) };
        }

        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, _s: Option<&AnyObject>) {
            self.choose(self.ivars().table.clickedRow());
        }
    }

    unsafe impl NSTableViewDataSource for BranchPicker {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _t: &NSTableView) -> NSInteger {
            self.ivars().list.borrow().items.len() as NSInteger
        }

    }

    unsafe impl NSTableViewDelegate for BranchPicker {
        /// Selected rows draw their text white, so both the row that lost the
        /// selection and the one that gained it are rebuilt.
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _n: &NSNotification) {
            let iv = self.ivars();
            let now = iv.table.selectedRow();
            let was = iv.selected.replace(now);
            let rows = NSMutableIndexSet::new();
            for r in [was, now].into_iter().filter(|r| *r >= 0) {
                rows.addIndex(r as usize);
            }
            iv.table
                .reloadDataForRowIndexes_columnIndexes(&rows, &NSIndexSet::indexSetWithIndex(0));
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(&self, t: &NSTableView, _c: Option<&NSTableColumn>, row: NSInteger) -> Option<Retained<NSView>> {
            self.make_row_view(t, row)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn row_view(&self, _t: &NSTableView, _row: NSInteger) -> Option<Retained<NSTableRowView>> {
            let mtm = MainThreadMarker::from(self);
            let v: Retained<PickerRow> = unsafe { msg_send![mtm.alloc::<PickerRow>(), init] };
            Some(Retained::into_super(v))
        }
    }

    unsafe impl NSControlTextEditingDelegate for BranchPicker {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            self.query_changed();
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        unsafe fn do_command(&self, _c: &NSControl, _tv: &NSTextView, command: Sel) -> bool {
            self.handle_command(command)
        }
    }

    unsafe impl NSTextFieldDelegate for BranchPicker {}

    unsafe impl NSPopoverDelegate for BranchPicker {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _n: &NSNotification) {
            // Only if this is still the current picker: a click that closes
            // one and opens another arrives before this notification.
            let mine = CURRENT
                .with(|c| {
                    let mut c = c.borrow_mut();
                    if c.as_deref().is_some_and(|p| std::ptr::eq(p, self)) {
                        c.take()
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    CLOSING.with(|c| {
                        let mut c = c.borrow_mut();
                        let i = c.iter().position(|p| std::ptr::eq(&**p, self))?;
                        Some(c.remove(i))
                    })
                });
            // Released on the next turn, not inside its own method.
            if let (Some(p), Some(mtm)) = (mine, MainThreadMarker::new()) {
                let p = MainThreadBound::new(p, mtm);
                DispatchQueue::main().exec_async(move || drop(p));
            }
            self.stop_watching();
            self.report_dismissal();
            // The popover took the keyboard; hand it back to the tree rather
            // than leaving the window with no first responder.
            if let Some(mtm) = MainThreadMarker::new() {
                crate::controller::focus_tree(mtm);
            }
        }
    }
);

define_class!(
    /// A row whose selection is always drawn in the accent colour: focus
    /// stays in the filter field, which would otherwise grey the bar out.
    #[unsafe(super(NSTableRowView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMPickerRow"]
    pub struct PickerRow;

    impl PickerRow {
        #[unsafe(method(isEmphasized))]
        fn is_emphasized(&self) -> bool {
            true
        }
    }
);

/// Bring the popover in line with the view's: open it, update it in place,
/// or close it.
pub fn render(popover: Option<&Popover>, mtm: MainThreadMarker) {
    let current = CURRENT.with(|c| c.borrow().clone());
    match (current, popover) {
        (Some(open), Some(p)) if open.ivars().id == p.id => open.update(&p.list),
        (open, p) => {
            if let Some(open) = open {
                open.close_by_view();
            }
            if let Some(p) = p {
                match crate::controller::anchor(&p.anchor.0, p.anchor.1, mtm) {
                    Some(anchor) => show(p, &anchor, mtm),
                    // Nothing to hang it from (the row went in the same
                    // render): as good as dismissed.
                    None => p.list.on_dismiss.call(()),
                }
            }
        }
    }
}

fn show(p: &Popover, anchor: &NSView, mtm: MainThreadMarker) {
    let field = NSTextField::textFieldWithString(&ns(&p.list.query), mtm);
    field.setPlaceholderString(Some(&ns(&p.list.placeholder)));
    field.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    // AppKit coordinates: y grows upwards, so the field sits above the list.
    field.setFrame(NSRect::new(
        NSPoint::new(PAD, 2.0 * PAD + LIST_HEIGHT),
        NSSize::new(WIDTH - 2.0 * PAD, 22.0),
    ));

    let scroll = NSScrollView::initWithFrame(
        mtm.alloc(),
        NSRect::new(
            NSPoint::new(PAD, PAD),
            NSSize::new(WIDTH - 2.0 * PAD, LIST_HEIGHT),
        ),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setBorderType(objc2_app_kit::NSBorderType::BezelBorder);
    let table = NSTableView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH - 2.0 * PAD, LIST_HEIGHT)),
    );
    let column = NSTableColumn::initWithIdentifier(mtm.alloc(), &ns("branch"));
    column.setWidth(WIDTH - 2.0 * PAD - 20.0);
    table.addTableColumn(&column);
    table.setHeaderView(None);
    table.setStyle(NSTableViewStyle::Plain);
    table.setRowHeight(ROW_HEIGHT);
    table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::Regular);
    table.setAllowsEmptySelection(true);
    table.setFocusRingType(objc2_app_kit::NSFocusRingType::None);
    table.setRefusesFirstResponder(true);
    table.setColumnAutoresizingStyle(
        objc2_app_kit::NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
    );
    scroll.setDocumentView(Some(&table));

    let container = NSView::initWithFrame(
        mtm.alloc(),
        NSRect::new(
            NSPoint::ZERO,
            NSSize::new(WIDTH, 3.0 * PAD + 22.0 + LIST_HEIGHT),
        ),
    );
    container.addSubview(&field);
    container.addSubview(&scroll);

    let vc = NSViewController::new(mtm);
    vc.setView(&container);
    let popover = NSPopover::new(mtm);
    // Not `Transient`: that closes on the mouse-down and lets the click reach
    // the button, whose action then reopens it on the mouse-up. Dismissal is
    // handled below instead, where a click on the button can be swallowed.
    popover.setBehavior(NSPopoverBehavior::ApplicationDefined);
    popover.setContentViewController(Some(&vc));
    popover.setContentSize(container.frame().size);

    let this = mtm.alloc::<BranchPicker>().set_ivars(BranchPickerIvars {
        id: p.id,
        popover: popover.clone(),
        field: field.clone(),
        table: table.clone(),
        list: RefCell::new(p.list.clone()),
        selected: Cell::new(-1),
        anchor: anchor.retain(),
        monitor: RefCell::new(None),
        closed_by_view: Cell::new(false),
    });
    let this: Retained<BranchPicker> = unsafe { msg_send![super(this), init] };
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(&*this)));
        table.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        table.setTarget(Some(this.as_ref()));
        table.setAction(Some(sel!(rowClicked:)));
        field.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    }
    popover.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    CURRENT.with(|c| *c.borrow_mut() = Some(this.clone()));

    table.reloadData();
    this.sync_selection();
    popover.showRelativeToRect_ofView_preferredEdge(
        anchor.bounds(),
        anchor,
        NSRectEdge::NSMaxYEdge,
    );
    this.watch_for_dismissal();
    if let Some(w) = field.window() {
        w.makeFirstResponder(Some(&field));
    }
}

impl BranchPicker {
    /// Close on the next click outside the popover. A click on the button
    /// that opened it is swallowed, so the picker toggles rather than
    /// closing and reopening on the same click.
    fn watch_for_dismissal(&self) {
        let mtm = MainThreadMarker::from(self);
        let me = MainThreadBound::new(self.retain(), mtm);
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let mtm = MainThreadMarker::new().expect("events arrive on the main thread");
            let event = unsafe { event.as_ref() };
            let me = me.get(mtm);
            let iv = me.ivars();
            if !iv.popover.isShown() {
                // Already on its way out; the click is not ours to swallow.
                me.stop_watching();
                return event as *const NSEvent as *mut NSEvent;
            }
            let popover_window = iv
                .popover
                .contentViewController()
                .and_then(|c| c.view().window());
            let clicked = event.window(mtm);
            if clicked.is_some() && clicked == popover_window {
                return event as *const NSEvent as *mut NSEvent;
            }
            let on_anchor = clicked.as_deref() == iv.anchor.window().as_deref()
                && iv.anchor.mouse_inRect(
                    iv.anchor
                        .convertPoint_fromView(event.locationInWindow(), None),
                    iv.anchor.bounds(),
                );
            unsafe { iv.popover.performClose(None) };
            // Now, not when the closing animation ends: a second click lands
            // in between, and it belongs to whatever it hits.
            me.stop_watching();
            if on_anchor {
                // The click has done its job; the button must not see it.
                std::ptr::null_mut()
            } else {
                event as *const NSEvent as *mut NSEvent
            }
        });
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
                &block,
            )
        };
        *self.ivars().monitor.borrow_mut() = monitor;
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(appResignedActive:),
                Some(NSApplicationDidResignActiveNotification),
                None,
            )
        };
    }

    fn stop_watching(&self) {
        if let Some(monitor) = self.ivars().monitor.borrow_mut().take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
        }
    }

    /// A recycled or new cell: a label pinned to the row's edges.
    fn cell_view(&self, table: &NSTableView) -> Retained<NSTableCellView> {
        let mtm = MainThreadMarker::from(self);
        if let Some(v) = unsafe { table.makeViewWithIdentifier_owner(&ns("wtm.pick"), None) } {
            if let Ok(c) = v.downcast::<NSTableCellView>() {
                return c;
            }
        }
        let cell = NSTableCellView::new(mtm);
        cell.setIdentifier(Some(&ns("wtm.pick")));
        let tf = label("", mtm);
        tf.setTranslatesAutoresizingMaskIntoConstraints(false);
        cell.addSubview(&tf);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            tf.leadingAnchor()
                .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 4.0),
            tf.trailingAnchor()
                .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -4.0),
            tf.centerYAnchor()
                .constraintEqualToAnchor(&cell.centerYAnchor()),
        ]));
        unsafe { cell.setTextField(Some(&tf)) };
        cell
    }

    fn make_row_view(&self, table: &NSTableView, row: NSInteger) -> Option<Retained<NSView>> {
        let label = self.label_for_row(row)?;
        let cell = self.cell_view(table);
        if let Some(tf) = unsafe { cell.textField() } {
            tf.setAttributedStringValue(&label);
        }
        Some(Retained::into_super(cell))
    }

    fn query_changed(&self) {
        let query = self.ivars().field.stringValue().to_string();
        let h = self.ivars().list.borrow().on_query.clone();
        h.call(query);
    }

    /// Escape, a click outside, or the app losing focus closed it.
    fn report_dismissal(&self) {
        if !self.ivars().closed_by_view.get() {
            let h = self.ivars().list.borrow().on_dismiss.clone();
            h.call(());
        }
    }

    fn close_by_view(&self) {
        self.ivars().closed_by_view.set(true);
        self.stop_watching();
        let mine = CURRENT.with(|c| {
            let mut c = c.borrow_mut();
            if c.as_deref().is_some_and(|p| std::ptr::eq(p, self)) {
                c.take()
            } else {
                None
            }
        });
        if let Some(p) = mine {
            CLOSING.with(|c| c.borrow_mut().push(p));
        }
        unsafe { self.ivars().popover.performClose(None) };
    }

    fn update(&self, list: &FilterList) {
        let iv = self.ivars();
        let items_changed = iv.list.borrow().items != list.items;
        *iv.list.borrow_mut() = list.clone();
        // Only when it differs: setting it moves the insertion point.
        if iv.field.stringValue().to_string() != list.query {
            iv.field.setStringValue(&ns(&list.query));
        }
        if items_changed {
            iv.selected.set(-1);
            iv.table.reloadData();
        }
        self.sync_selection();
    }

    fn sync_selection(&self) {
        let iv = self.ivars();
        let want = iv.list.borrow().selected.map_or(-1, |s| s as NSInteger);
        if iv.table.selectedRow() == want {
            return;
        }
        if want < 0 {
            unsafe { iv.table.deselectAll(None) };
        } else {
            iv.table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(want as usize),
                false,
            );
            iv.table.scrollRowToVisible(want);
        }
    }

    fn choose(&self, row: NSInteger) {
        if row < 0 || row as usize >= self.ivars().list.borrow().items.len() {
            return;
        }
        let h = self.ivars().list.borrow().on_choose.clone();
        h.call(row as usize);
    }

    fn handle_command(&self, command: Sel) -> bool {
        let list = self.ivars().list.borrow().clone();
        if command == sel!(moveDown:) {
            list.on_move.call(1);
            true
        } else if command == sel!(moveUp:) {
            list.on_move.call(-1);
            true
        } else if command == sel!(insertNewline:) {
            if let Some(i) = list.selected {
                list.on_choose.call(i);
            }
            true
        } else if command == sel!(cancelOperation:) {
            self.stop_watching();
            unsafe { self.ivars().popover.performClose(None) };
            true
        } else {
            false
        }
    }

    /// The row's text: a check mark for the current branch, the name with an
    /// agent prefix drawn as its mark, and the matched characters emphasised.
    /// A selected row's text is white, so it is rebuilt when selection moves.
    fn label_for_row(&self, row: NSInteger) -> Option<Retained<NSAttributedString>> {
        let iv = self.ivars();
        let item = iv.list.borrow().items.get(row as usize)?.clone();
        let selected = iv.table.selectedRow() == row;
        let ink = if selected {
            NSColor::alternateSelectedControlTextColor()
        } else {
            crate::util::primary_ink()
        };
        let font = crate::util::branch_font();
        let bold = NSFont::monospacedSystemFontOfSize_weight(12.0, SEMIBOLD);
        let label = rich_label(&item.label, &font, &ink, Some(&bold));
        let text = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &label,
        );
        let is_current = item.checked;
        let attrs = unsafe {
            NSDictionary::from_retained_objects::<NSString>(
                &[NSFontAttributeName, NSForegroundColorAttributeName],
                &[
                    Retained::into_super(Retained::into_super(font)),
                    Retained::into_super(Retained::into_super(ink)),
                ],
            )
        };
        let check = unsafe {
            NSMutableAttributedString::initWithString_attributes(
                NSMutableAttributedString::alloc(),
                &ns(if is_current { "✓ " } else { "   " }),
                Some(&attrs),
            )
        };
        text.insertAttributedString_atIndex(&check, 0);
        Some(Retained::into_super(text))
    }
}
