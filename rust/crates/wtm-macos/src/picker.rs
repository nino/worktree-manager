//! The branch picker: a popover under a row's branch button with a filter
//! field above a fuzzy-matched, keyboard-navigable list, like the Electron
//! app's. Typing filters and ranks (`wtm_core::fuzzy`), ↑/↓ move, Return
//! chooses, Escape or a click outside closes. Works for detached worktrees
//! too, which the old popup could not offer.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSControl, NSControlTextEditingDelegate, NSFont, NSFontAttributeName, NSLayoutConstraint,
    NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSScrollView, NSTableCellView, NSTableColumn,
    NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField, NSTextFieldDelegate,
    NSTextView, NSUserInterfaceItemIdentification, NSView, NSViewController,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSIndexSet, NSInteger, NSMutableAttributedString,
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSRectEdge, NSSize,
    NSString,
};
use wtm_core::fuzzy::{fuzzy_filter, Match};

use crate::util::{label, ns, REGULAR, SEMIBOLD};

const WIDTH: f64 = 300.0;
const LIST_HEIGHT: f64 = 208.0;
const PAD: f64 = 8.0;
const ROW_HEIGHT: f64 = 22.0;

thread_local! {
    /// The open picker, if any: the popover keeps no strong reference to
    /// its delegate and data source.
    static CURRENT: RefCell<Option<Retained<BranchPicker>>> = const { RefCell::new(None) };
}

pub struct BranchPickerIvars {
    popover: Retained<NSPopover>,
    field: Retained<NSTextField>,
    table: Retained<NSTableView>,
    all: Vec<String>,
    current: Option<String>,
    filtered: RefCell<Vec<(usize, Match)>>,
    on_choose: Box<dyn Fn(String)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WTMBranchPicker"]
    #[ivars = BranchPickerIvars]
    pub struct BranchPicker;

    unsafe impl NSObjectProtocol for BranchPicker {}

    impl BranchPicker {
        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, _s: Option<&AnyObject>) {
            self.choose(self.ivars().table.clickedRow());
        }
    }

    unsafe impl NSTableViewDataSource for BranchPicker {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _t: &NSTableView) -> NSInteger {
            self.ivars().filtered.borrow().len() as NSInteger
        }

    }

    unsafe impl NSTableViewDelegate for BranchPicker {
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
            self.refilter();
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
            CURRENT.with(|c| c.borrow_mut().take());
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

/// Open the picker under `anchor`. `on_choose` runs with the chosen branch
/// unless it is the current one.
pub fn show(
    anchor: &NSView,
    branches: &[String],
    current: Option<&str>,
    on_choose: impl Fn(String) + 'static,
) {
    let mtm = MainThreadMarker::from(anchor);
    close();
    // The current branch stays choosable even if the list is stale.
    let mut all: Vec<String> = Vec::with_capacity(branches.len() + 1);
    if let Some(c) = current.filter(|c| !branches.iter().any(|b| b == c)) {
        all.push(c.to_string());
    }
    all.extend(branches.iter().cloned());

    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    field.setPlaceholderString(Some(&ns("e.g., main")));
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
    popover.setBehavior(NSPopoverBehavior::Transient);
    popover.setContentViewController(Some(&vc));
    popover.setContentSize(container.frame().size);

    let this = mtm.alloc::<BranchPicker>().set_ivars(BranchPickerIvars {
        popover: popover.clone(),
        field: field.clone(),
        table: table.clone(),
        all,
        current: current.map(str::to_string),
        filtered: RefCell::new(Vec::new()),
        on_choose: Box::new(on_choose),
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

    this.refilter();
    // Start on the current branch, as a menu would.
    if let Some(row) = this.row_of_current() {
        this.select(row as NSInteger);
    }
    popover.showRelativeToRect_ofView_preferredEdge(
        anchor.bounds(),
        anchor,
        NSRectEdge::NSMaxYEdge,
    );
    if let Some(w) = field.window() {
        w.makeFirstResponder(Some(&field));
    }
}

/// Close the open picker, if any.
pub fn close() {
    if let Some(p) = CURRENT.with(|c| c.borrow().clone()) {
        unsafe { p.ivars().popover.performClose(None) };
    }
}

impl BranchPicker {
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

    fn refilter(&self) {
        let iv = self.ivars();
        let query = iv.field.stringValue().to_string();
        *iv.filtered.borrow_mut() = fuzzy_filter(&query, &iv.all);
        iv.table.reloadData();
        if iv.table.numberOfRows() > 0 {
            self.select(0);
        }
    }

    fn row_of_current(&self) -> Option<usize> {
        let iv = self.ivars();
        let current = iv.current.as_deref()?;
        iv.filtered
            .borrow()
            .iter()
            .position(|(i, _)| iv.all[*i] == current)
    }

    fn select(&self, row: NSInteger) {
        let table = &self.ivars().table;
        if row < 0 || row >= table.numberOfRows() {
            return;
        }
        table.selectRowIndexes_byExtendingSelection(
            &NSIndexSet::indexSetWithIndex(row as usize),
            false,
        );
        table.scrollRowToVisible(row);
    }

    fn choose(&self, row: NSInteger) {
        let iv = self.ivars();
        let branch = {
            let filtered = iv.filtered.borrow();
            if row < 0 || row as usize >= filtered.len() {
                return;
            }
            iv.all[filtered[row as usize].0].clone()
        };
        unsafe { iv.popover.performClose(None) };
        if iv.current.as_deref() != Some(branch.as_str()) {
            (iv.on_choose)(branch);
        }
    }

    fn handle_command(&self, command: Sel) -> bool {
        let table = &self.ivars().table;
        let selected = table.selectedRow();
        if command == sel!(moveDown:) {
            self.select(selected + 1);
            true
        } else if command == sel!(moveUp:) {
            self.select((selected - 1).max(0));
            true
        } else if command == sel!(insertNewline:) {
            self.choose(selected);
            true
        } else if command == sel!(cancelOperation:) {
            unsafe { self.ivars().popover.performClose(None) };
            true
        } else {
            false
        }
    }

    /// The row's text: a check mark for the current branch, the name in
    /// monospace with the matched characters emphasised.
    fn label_for_row(&self, row: NSInteger) -> Option<Retained<NSAttributedString>> {
        let iv = self.ivars();
        let filtered = iv.filtered.borrow();
        let (index, m) = filtered.get(row as usize)?;
        let name = &iv.all[*index];
        let is_current = iv.current.as_deref() == Some(name.as_str());
        let prefix = if is_current { "✓ " } else { "   " };
        let text = format!("{prefix}{name}");
        let font = NSFont::monospacedSystemFontOfSize_weight(12.0, REGULAR);
        let bold = NSFont::monospacedSystemFontOfSize_weight(12.0, SEMIBOLD);
        let attrs = unsafe {
            NSDictionary::from_retained_objects::<NSString>(
                &[NSFontAttributeName],
                &[Retained::into_super(Retained::into_super(font))],
            )
        };
        let s = unsafe {
            NSMutableAttributedString::initWithString_attributes(
                NSMutableAttributedString::alloc(),
                &ns(&text),
                Some(&attrs),
            )
        };
        // Positions are char indices into the name; the string is UTF-16.
        let utf16_offsets: Vec<usize> = name
            .chars()
            .scan(prefix.encode_utf16().count(), |acc, c| {
                let here = *acc;
                *acc += c.len_utf16();
                Some(here)
            })
            .collect();
        let bold_attrs = unsafe {
            NSDictionary::from_retained_objects::<NSString>(
                &[NSFontAttributeName],
                &[Retained::into_super(Retained::into_super(bold))],
            )
        };
        for p in &m.positions {
            if let Some(&off) = utf16_offsets.get(*p) {
                let len = name.chars().nth(*p).map(char::len_utf16).unwrap_or(1);
                unsafe { s.addAttributes_range(&bold_attrs, NSRange::new(off, len)) };
            }
        }
        Some(Retained::into_super(s))
    }
}
