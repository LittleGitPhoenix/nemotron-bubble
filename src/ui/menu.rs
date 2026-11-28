//! Popup menus in the colors of the application window. They stay native Win32 menus
//! (keyboard, submenus and closing work as usual), but every entry is owner-drawn:
//! Windows asks for its size with WM_MEASUREITEM and lets the app paint it with WM_DRAWITEM.
//! Only the thin outer frame and the shadow are still drawn by Windows.

use crate::*;
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, ExcludeClipRect, HBRUSH};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_CHECKED, ODS_DISABLED, ODS_GRAYED, ODS_SELECTED, ODT_MENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MENU_ITEM_FLAGS, MENUINFO, MF_OWNERDRAW, MIM_APPLYTOSUBMENUS, MIM_BACKGROUND, SetMenuInfo,
};

const MENU_BACKGROUND: Rgb = (40, 44, 54);
const MENU_HIGHLIGHT: Rgb = (58, 64, 78);
const MENU_SEPARATOR: Rgb = (60, 66, 78);
const MENU_ITEM_HEIGHT: i32 = 28;
const MENU_SEPARATOR_HEIGHT: i32 = 9;
// The text starts right of the check mark column.
const MENU_TEXT_LEFT: i32 = 30;
const MENU_TEXT_RIGHT_PADDING: i32 = 16;
// Room for the chevron of an entry that opens a submenu.
const MENU_SUBMENU_ARROW_WIDTH: i32 = 22;
// Segoe MDL2 Assets glyphs.
const GLYPH_CHECK_MARK: &str = "\u{E73E}";
const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";

static MENU_BACKGROUND_BRUSH: OnceCell<isize> = OnceCell::new();

/// What an owner-drawn entry shows. Windows hands its address back in WM_MEASUREITEM
/// and WM_DRAWITEM as the item data.
enum MenuEntry {
    /// With `accent`, the text is drawn in the accent color and the given icon-font
    /// glyph sits in the check mark column.
    Item { text: String, opens_submenu: bool, accent: Option<&'static str> },
    Separator,
}

thread_local! {
    // The entries of the menus built since the last menu closed. Boxed, so their
    // addresses stay valid while the list grows. Cleared by `track_popup_menu`.
    static MENU_ENTRIES: RefCell<Vec<Box<MenuEntry>>> = const { RefCell::new(Vec::new()) };
}

fn register_entry(entry: MenuEntry) -> *const MenuEntry {
    MENU_ENTRIES.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.push(Box::new(entry));
        &**entries.last().expect("just pushed") as *const MenuEntry
    })
}

/// Append an owner-drawn entry. `flags` may contain MF_CHECKED, MF_GRAYED and MF_POPUP.
/// With MF_POPUP, `id` is the submenu handle like for AppendMenuW. `text` is drawn as
/// it is, so "&" needs no escaping.
pub(crate) unsafe fn append_menu_item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let entry = register_entry(MenuEntry::Item {
        text: text.to_string(),
        opens_submenu: flags & MF_POPUP != 0,
        accent: None,
    });
    AppendMenuW(menu, flags | MF_OWNERDRAW, id, entry as *const u16);
}

/// Append an owner-drawn entry that stands out: accent-colored text with the
/// icon-font `glyph` in front of it, e.g. the tray menu's update entry.
pub(crate) unsafe fn append_accent_menu_item(menu: HMENU, id: usize, text: &str, glyph: &'static str) {
    let entry = register_entry(MenuEntry::Item {
        text: text.to_string(),
        opens_submenu: false,
        accent: Some(glyph),
    });
    AppendMenuW(menu, MF_STRING | MF_OWNERDRAW, id, entry as *const u16);
}

/// Append an owner-drawn separator line.
pub(crate) unsafe fn append_menu_separator(menu: HMENU) {
    let entry = register_entry(MenuEntry::Separator);
    AppendMenuW(menu, MF_SEPARATOR | MF_OWNERDRAW, 0, entry as *const u16);
}

/// Show `menu` at the screen point (x, y) for the owner window `hwnd`, which receives
/// WM_MEASUREITEM, WM_DRAWITEM and the chosen WM_COMMAND. Destroys the menu with its
/// submenus afterwards.
pub(crate) unsafe fn track_popup_menu(menu: HMENU, flags: TRACK_POPUP_MENU_FLAGS, x: i32, y: i32, hwnd: HWND) {
    let brush = *MENU_BACKGROUND_BRUSH
        .get_or_init(|| CreateSolidBrush(rgb(MENU_BACKGROUND.0, MENU_BACKGROUND.1, MENU_BACKGROUND.2)) as isize);
    let info = MENUINFO {
        cbSize: std::mem::size_of::<MENUINFO>() as u32,
        fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS,
        hbrBack: brush as HBRUSH,
        ..std::mem::zeroed()
    };
    SetMenuInfo(menu, &info);
    TrackPopupMenu(menu, flags, x, y, 0, hwnd, null());
    DestroyMenu(menu);
    MENU_ENTRIES.with(|entries| entries.borrow_mut().clear());
}

/// Width of `text` in the menu font.
unsafe fn text_width(text: &str) -> i32 {
    text_width_in(ui_font(), text)
}

/// WM_MEASUREITEM: the size of an owner-drawn menu entry. Returns false for messages
/// that are not about one of these menus.
pub(crate) unsafe fn measure_menu_item(lparam: LPARAM) -> bool {
    let measure = &mut *(lparam as *mut MEASUREITEMSTRUCT);
    if measure.CtlType != ODT_MENU || measure.itemData == 0 {
        return false;
    }
    match &*(measure.itemData as *const MenuEntry) {
        MenuEntry::Item { text, opens_submenu, .. } => {
            let arrow = if *opens_submenu { MENU_SUBMENU_ARROW_WIDTH } else { 0 };
            // Windows adds the width of a check mark itself, so leave that out.
            let check = GetSystemMetrics(SM_CXMENUCHECK) - 1;
            let width = MENU_TEXT_LEFT + text_width(text) + MENU_TEXT_RIGHT_PADDING + arrow - check;
            measure.itemWidth = width.max(0) as u32;
            measure.itemHeight = MENU_ITEM_HEIGHT as u32;
        }
        MenuEntry::Separator => {
            measure.itemWidth = 0;
            measure.itemHeight = MENU_SEPARATOR_HEIGHT as u32;
        }
    }
    true
}

/// WM_DRAWITEM: paint an owner-drawn menu entry. Returns false for messages that are
/// not about one of these menus.
pub(crate) unsafe fn draw_menu_item(lparam: LPARAM) -> bool {
    let draw = &*(lparam as *const DRAWITEMSTRUCT);
    if draw.CtlType != ODT_MENU || draw.itemData == 0 {
        return false;
    }
    let rect = draw.rcItem;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let Some(mut canvas) = Canvas::new(width, height) else {
        return true;
    };
    let entry = &*(draw.itemData as *const MenuEntry);
    let disabled = draw.itemState & (ODS_GRAYED | ODS_DISABLED) != 0;
    let highlighted = draw.itemState & ODS_SELECTED != 0 && !disabled;

    // Shapes into the pixel buffer, then the texts with GDI on top of it.
    {
        let buf = canvas.pixels();
        // Opaque pixels are 0xAARRGGBB.
        let (bg_r, bg_g, bg_b) = MENU_BACKGROUND;
        buf.fill(0xFF00_0000 | (bg_r as u32) << 16 | (bg_g as u32) << 8 | bg_b as u32);
        match entry {
            MenuEntry::Item { .. } if highlighted => {
                let (w, h) = (width as f32, height as f32);
                fill_round_rect(buf, width, height, 4.0, 1.0, w - 8.0, h - 2.0, 5.0, MENU_HIGHLIGHT, MENU_HIGHLIGHT, 1.0);
            }
            MenuEntry::Separator => {
                let y = (height / 2) as f32;
                fill_round_rect(buf, width, height, 8.0, y, width as f32 - 16.0, 1.0, 0.0, MENU_SEPARATOR, MENU_SEPARATOR, 1.0);
            }
            MenuEntry::Item { .. } => {}
        }
    }
    if let MenuEntry::Item { text, opens_submenu, accent } = entry {
        let dc = canvas.hdc;
        SetBkMode(dc, TRANSPARENT as i32);
        let full = RectI {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        if draw.itemState & ODS_CHECKED != 0 {
            SelectObject(dc, icon_font() as _);
            SetTextColor(dc, rgb(110, 162, 255));
            text_center(dc, GLYPH_CHECK_MARK, RectI { right: MENU_TEXT_LEFT, ..full });
        }
        if let Some(glyph) = accent {
            SelectObject(dc, icon_font() as _);
            SetTextColor(dc, rgb(110, 162, 255));
            text_center(dc, glyph, RectI { right: MENU_TEXT_LEFT, ..full });
        }
        SelectObject(dc, ui_font() as _);
        let color = match (disabled, accent) {
            (true, _) => rgb(124, 134, 148),
            (false, Some(_)) => rgb(110, 162, 255),
            (false, None) => rgb(233, 238, 244),
        };
        SetTextColor(dc, color);
        let text_right = width - MENU_TEXT_RIGHT_PADDING + 8 - if *opens_submenu { MENU_SUBMENU_ARROW_WIDTH } else { 0 };
        text_left_ellipsis(dc, text, RectI { left: MENU_TEXT_LEFT, right: text_right, ..full });
        if *opens_submenu {
            SelectObject(dc, icon_font() as _);
            SetTextColor(dc, if disabled { rgb(124, 134, 148) } else { rgb(170, 178, 192) });
            text_center(dc, GLYPH_CHEVRON_RIGHT, RectI { left: width - MENU_SUBMENU_ARROW_WIDTH - 4, right: width - 4, ..full });
        }
    }
    canvas.blit_to_at(draw.hDC, rect.left, rect.top);
    // Windows draws its own submenu arrow after this message. Excluding the entry from
    // the clip region keeps that black arrow off the dark background.
    ExcludeClipRect(draw.hDC, rect.left, rect.top, rect.right, rect.bottom);
    true
}
