//! Drawing toolkit: the off-screen canvas, anti-aliased shapes, text helpers and fonts.

use crate::*;

pub(crate) static UI_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static TITLE_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static SMALL_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static ICON_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static ICON_FONT_LARGE: OnceCell<isize> = OnceCell::new();
pub(crate) static TINY_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static HEADING_FONT: OnceCell<isize> = OnceCell::new();
pub(crate) static BADGE_FONT: OnceCell<isize> = OnceCell::new();
#[derive(Copy, Clone)]
pub(crate) struct RectI {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl RectI {
    /// The same rectangle moved down by `dy` (up when negative).
    pub(crate) fn shifted(self, dy: i32) -> RectI {
        RectI {
            top: self.top + dy,
            bottom: self.bottom + dy,
            ..self
        }
    }

    pub(crate) fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

pub(crate) struct Canvas {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) hdc: HDC,
    pub(crate) bitmap: HBITMAP,
    pub(crate) old_bitmap: isize,
    pub(crate) bits: *mut u32,
}

impl Canvas {
    pub(crate) unsafe fn new(w: i32, h: i32) -> Option<Canvas> {
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(null_mut());
        let hdc = CreateCompatibleDC(screen);
        ReleaseDC(null_mut(), screen);
        if hdc == null_mut() {
            return None;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // negative => top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB as u32;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap == null_mut() || bits == null_mut() {
            DeleteDC(hdc);
            return None;
        }
        let old_bitmap = SelectObject(hdc, bitmap as _) as isize;
        Some(Canvas {
            w,
            h,
            hdc,
            bitmap,
            old_bitmap,
            bits: bits as *mut u32,
        })
    }

    pub(crate) fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }

    pub(crate) fn clear_transparent(&mut self) {
        for p in self.pixels() {
            *p = 0;
        }
    }

    /// Push the buffer to a per-pixel-alpha layered window. Position is left
    /// unchanged (so dragging via SetWindowPos keeps working).
    pub(crate) unsafe fn present_layered(&self, hwnd: HWND) {
        GdiFlush();
        let screen = GetDC(null_mut());
        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            hwnd,
            screen,
            null(),
            &size,
            self.hdc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        );
        if screen != null_mut() {
            ReleaseDC(null_mut(), screen);
        }
    }

    /// Copy the buffer to a destination DC (opaque double-buffer blit).
    pub(crate) unsafe fn blit_to(&self, dst: HDC) {
        self.blit_to_at(dst, 0, 0);
    }

    /// Copy the buffer to a destination DC with its top left corner at (x, y).
    pub(crate) unsafe fn blit_to_at(&self, dst: HDC, x: i32, y: i32) {
        GdiFlush();
        BitBlt(dst, x, y, self.w, self.h, self.hdc, 0, 0, SRCCOPY);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old_bitmap as _);
            DeleteObject(self.bitmap as _);
            DeleteDC(self.hdc);
        }
    }
}

pub(crate) type Rgb = (u8, u8, u8);

#[inline]
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Premultiplied source-over blend of a straight-alpha colour onto one pixel.
#[inline]
pub(crate) fn blend_px(buf: &mut [u32], idx: usize, r: f32, g: f32, b: f32, a: f32) {
    if a <= 0.0 {
        return;
    }
    let a = a.min(1.0);
    let d = buf[idx];
    let da = ((d >> 24) & 0xff) as f32;
    let dr = ((d >> 16) & 0xff) as f32;
    let dg = ((d >> 8) & 0xff) as f32;
    let db = (d & 0xff) as f32;
    let inv = 1.0 - a;
    let or = r * a + dr * inv;
    let og = g * a + dg * inv;
    let ob = b * a + db * inv;
    let oa = a * 255.0 + da * inv;
    buf[idx] = ((oa as u32) << 24) | ((or as u32) << 16) | ((og as u32) << 8) | (ob as u32);
}

/// Distance from a point to the line segment (ax, ay)-(bx, by). Subtract a
/// radius to get a capsule (a line with round caps).
#[inline]
pub(crate) fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay) = (px - ax, py - ay);
    let (bax, bay) = (bx - ax, by - ay);
    let len2 = (bax * bax + bay * bay).max(f32::EPSILON);
    let h = ((pax * bax + pay * bay) / len2).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h)
}

/// Anti-aliased fill of any shape given by a signed distance function (negative
/// inside), evaluated for the pixels of the box (x0, y0)-(x1, y1).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_sdf<F: Fn(f32, f32) -> f32>(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    color: Rgb,
    alpha: f32,
    sdf: F,
) {
    let px0 = (x0.floor() as i32 - 1).max(0);
    let py0 = (y0.floor() as i32 - 1).max(0);
    let px1 = (x1.ceil() as i32 + 1).min(cw);
    let py1 = (y1.ceil() as i32 + 1).min(ch);
    for py in py0..py1 {
        let fy = py as f32 + 0.5;
        let row = (py * cw) as usize;
        for px in px0..px1 {
            let cov = (0.5 - sdf(px as f32 + 0.5, fy)).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_px(buf, row + px as usize, color.0 as f32, color.1 as f32, color.2 as f32, cov * alpha);
            }
        }
    }
}

/// Signed distance from a point to a rounded rectangle (negative inside).
#[inline]
pub(crate) fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

/// Anti-aliased rounded rectangle with an optional vertical colour gradient.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_round_rect(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    x: f32,
    y: f32,
    rw: f32,
    rh: f32,
    radius: f32,
    top: Rgb,
    bot: Rgb,
    alpha: f32,
) {
    if rw <= 0.0 || rh <= 0.0 {
        return;
    }
    let cx = x + rw / 2.0;
    let cy = y + rh / 2.0;
    let hx = rw / 2.0;
    let hy = rh / 2.0;
    let r = radius.min(hx).min(hy).max(0.0);
    let x0 = (x.floor() as i32 - 1).max(0);
    let y0 = (y.floor() as i32 - 1).max(0);
    let x1 = ((x + rw).ceil() as i32 + 1).min(cw);
    let y1 = ((y + rh).ceil() as i32 + 1).min(ch);
    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let t = ((fy - y) / rh).clamp(0.0, 1.0);
        let cr = lerp(top.0 as f32, bot.0 as f32, t);
        let cg = lerp(top.1 as f32, bot.1 as f32, t);
        let cb = lerp(top.2 as f32, bot.2 as f32, t);
        let row = (py * cw) as usize;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let d = sd_round_rect(fx, fy, cx, cy, hx, hy, r);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_px(buf, row + px as usize, cr, cg, cb, cov * alpha);
            }
        }
    }
}

/// Soft drop shadow: a rounded-rect silhouette whose alpha falls off over `blur`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_shadow(
    buf: &mut [u32],
    cw: i32,
    ch: i32,
    x: f32,
    y: f32,
    rw: f32,
    rh: f32,
    radius: f32,
    blur: f32,
    max_alpha: f32,
) {
    let cx = x + rw / 2.0;
    let cy = y + rh / 2.0;
    let hx = rw / 2.0;
    let hy = rh / 2.0;
    let r = radius.min(hx).min(hy).max(0.0);
    let x0 = ((x - blur).floor() as i32).max(0);
    let y0 = ((y - blur).floor() as i32).max(0);
    let x1 = ((x + rw + blur).ceil() as i32).min(cw);
    let y1 = ((y + rh + blur).ceil() as i32).min(ch);
    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        let row = (py * cw) as usize;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let d = sd_round_rect(fx, fy, cx, cy, hx, hy, r).max(0.0);
            let cov = (1.0 - d / blur).clamp(0.0, 1.0);
            if cov > 0.0 {
                // ease the falloff so the shadow looks soft, not linear
                let a = cov * cov * max_alpha;
                blend_px(buf, row + px as usize, 0.0, 0.0, 0.0, a);
            }
        }
    }
}

pub(crate) unsafe fn text_left(dc: HDC, text: &str, x: i32, y: i32, w: i32, h: i32) {
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX,
    );
}

pub(crate) unsafe fn text_right(dc: HDC, text: &str, rect: RectI) {
    let mut r = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let wide = wide_null(text);
    DrawTextW(dc, wide.as_ptr(), -1, &mut r, DT_RIGHT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
}

pub(crate) unsafe fn text_center(dc: HDC, text: &str, rect: RectI) {
    let mut r = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut r,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
}

pub(crate) unsafe fn create_font(height: i32, weight: i32) -> isize {
    create_font_face("Segoe UI", height, weight)
}

pub(crate) unsafe fn create_font_face(face: &str, height: i32, weight: i32) -> isize {
    let face = wide_null(face);
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        face.as_ptr(),
    ) as isize
}

/// Windows' icon font (Windows 10 and 11), for the title-bar style close glyph.
pub(crate) fn icon_font() -> isize {
    *ICON_FONT.get_or_init(|| unsafe { create_font_face("Segoe MDL2 Assets", -11, FW_NORMAL as i32) })
}

pub(crate) fn ui_font() -> isize {
    *UI_FONT.get_or_init(|| unsafe { create_font(-15, FW_NORMAL as i32) })
}

pub(crate) fn title_font() -> isize {
    *TITLE_FONT.get_or_init(|| unsafe { create_font(-25, FW_SEMIBOLD as i32) })
}

pub(crate) fn small_font() -> isize {
    *SMALL_FONT.get_or_init(|| unsafe { create_font(-12, FW_NORMAL as i32) })
}

/// Header lines of history entries (date, device, time per chunk).
pub(crate) fn tiny_font() -> isize {
    *TINY_FONT.get_or_init(|| unsafe { create_font(-11, FW_NORMAL as i32) })
}

/// Larger icon-font glyphs, e.g. the trash bin.
pub(crate) fn icon_font_large() -> isize {
    *ICON_FONT_LARGE.get_or_init(|| unsafe { create_font_face("Segoe MDL2 Assets", -15, FW_NORMAL as i32) })
}

pub(crate) fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}


/// Single line, left-aligned and vertically centered in `rect`, cut with "..."
/// when it does not fit.
pub(crate) unsafe fn text_left_ellipsis(dc: HDC, text: &str, rect: RectI) {
    let mut r = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let wide = wide_null(text);
    DrawTextW(
        dc,
        wide.as_ptr(),
        -1,
        &mut r,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

/// Semi-bold headings, e.g. the title of the model notice.
pub(crate) fn heading_font() -> isize {
    *HEADING_FONT.get_or_init(|| unsafe { create_font(-15, FW_SEMIBOLD as i32) })
}

/// The bold numbers and letters inside the step badges of the model notice.
pub(crate) fn badge_font() -> isize {
    *BADGE_FONT.get_or_init(|| unsafe { create_font(-11, FW_BOLD as i32) })
}
