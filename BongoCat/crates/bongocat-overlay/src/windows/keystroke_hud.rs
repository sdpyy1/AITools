//! The keystroke counter drawn beside the cat.
//!
//! The overlay is a D3D11 surface owned by `renderer`, and a layered window's
//! children are not composited into it, so the counter is a second top-level
//! window instead: its own small layered HWND, moved to the overlay's
//! lower-left corner whenever the overlay moves and repainted only while a
//! "+1" flourish is still travelling. It is painted with the same alpha the
//! overlay was handed this frame, so it fades out together with the cat —
//! hover, idle and fullscreen hides included — rather than leaving a number
//! floating over an empty desktop.
//!
//! The glyphs come from a hard-coded 5x7 bitmap rather than a system font: the
//! whole vocabulary is ten digits and a plus sign, drawing them keeps them
//! crisp at this size, and it avoids the dark fringing GDI text produces when
//! it is composited against a transparent background.

use super::*;

use std::{collections::VecDeque, ffi::c_void};

use windows::Win32::Foundation::{COLORREF, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, RGBQUAD, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SWP_SHOWWINDOW, ULW_ALPHA, UpdateLayeredWindow,
};

/// The window class this counter registers, kept process-wide so a crashed run
/// cannot leave two of them behind.
const HUD_CLASS: ::windows::core::PCWSTR = w!("BongoCatKeystrokeCounter");

/// How large the counter may grow before it stops widening: six digits plus two
/// margins is already wider than the number has any business being.
const MAX_COUNT_DIGITS: usize = 6;
/// Device-independent size of one bitmap pixel of a glyph.
const GLYPH_SCALE: i32 = 3;
const GLYPH_COLS: i32 = 5;
const GLYPH_ROWS: i32 = 7;
const GLYPH_WIDTH: i32 = GLYPH_COLS * GLYPH_SCALE;
const GLYPH_HEIGHT: i32 = GLYPH_ROWS * GLYPH_SCALE;
/// Space between two glyphs, which is one glyph pixel at this scale.
const GLYPH_ADVANCE: i32 = GLYPH_WIDTH + GLYPH_SCALE;
const HUD_MARGIN: i32 = 6;
/// How far a flourish rises before it disappears.
const POP_TRAVEL: i32 = 28;
const HUD_WIDTH: i32 = MAX_COUNT_DIGITS as i32 * GLYPH_ADVANCE + HUD_MARGIN * 2;
const HUD_HEIGHT: i32 = POP_TRAVEL + GLYPH_HEIGHT + HUD_MARGIN * 2;
/// Width of the two-glyph "+1" flourish, used to centre it over the number.
const FLOURISH_WIDTH: i32 = GLYPH_ADVANCE * 2 - GLYPH_SCALE;

/// How long a "+1" stays on screen.
const POP_LIFETIME: Duration = Duration::from_millis(750);
/// The newest flourishes kept: a burst of typing would otherwise stack them far
/// faster than they can be read.
const MAX_ACTIVE_POPS: usize = 6;

/// 5x7 glyphs, one byte per row, the most significant bit being the left column.
const DIGIT_GLYPHS: [[u8; GLYPH_ROWS as usize]; 10] = [
    [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E], // 0
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E], // 1
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F], // 2
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E], // 3
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02], // 4
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E], // 5
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E], // 6
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08], // 7
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C], // 9
];
const PLUS_GLYPH: [u8; GLYPH_ROWS as usize] = [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00];

/// The glyph is written white over a black outline so it stays legible against
/// both a light desktop and a dark game.
const GLYPH_FILL: (u8, u8, u8) = (255, 255, 255);
const GLYPH_OUTLINE: (u8, u8, u8) = (0, 0, 0);

/// One travelling "+1".
struct Flourish {
    born: Instant,
    /// Horizontal offset so a burst of presses does not stack them in a column.
    jitter: i32,
}

/// The counter window. Created once and kept alongside the session, because it
/// has to outlive every replacement of the model window.
pub(crate) struct KeystrokeHud {
    hwnd: HWND,
    flourishes: VecDeque<Flourish>,
    last_total: u64,
    visible: bool,
    last_origin: Option<(i32, i32)>,
}

/// Nothing about the counter is interactive, so the default procedure is all it
/// needs: every visual change is pushed with `UpdateLayeredWindow` rather than
/// arriving as a paint message.
unsafe extern "system" fn hud_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: `hwnd` is the live HWND this procedure was registered against.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

impl KeystrokeHud {
    /// Create the counter window, hidden.
    pub(crate) fn create() -> Result<Self, OverlayError> {
        // SAFETY: the class and the HWND below are created and used only on the
        // thread that creates them, and no borrowed Win32 pointer escapes this
        // owner.
        let module = unsafe { GetModuleHandleW(None) }
            .map_err(windows_error("resolve the keystroke counter instance"))?;
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(hud_window_proc),
            hInstance: instance,
            lpszClassName: HUD_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            let error = Error::from_thread();
            // A class left behind by an earlier crash in this process is the same
            // class; the counter can still be created from it.
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(windows_error("register the keystroke counter class")(
                    error,
                ));
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                HUD_CLASS,
                w!(""),
                WS_POPUP,
                0,
                0,
                HUD_WIDTH,
                HUD_HEIGHT,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map_err(windows_error("create the keystroke counter window"))?;
        Ok(Self {
            hwnd,
            flourishes: VecDeque::new(),
            last_total: bongocat_platform::key_press_total(),
            visible: false,
            last_origin: None,
        })
    }

    /// Move the counter with the overlay and repaint it.
    ///
    /// `alpha` is the overlay's own opacity for this frame, so the counter is
    /// never the one thing left visible after the cat has faded out. A hidden
    /// counter stops being repainted entirely, which is what keeps it off the
    /// desktop rather than merely invisible.
    pub(crate) fn update(
        &mut self,
        bounds: OverlayWindowBounds,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        let total = bongocat_platform::key_press_total();
        if total > self.last_total {
            for index in 0..(total - self.last_total).min(MAX_ACTIVE_POPS as u64) {
                self.flourishes.push_back(Flourish {
                    born: Instant::now(),
                    jitter: (index as i32 % 3 - 1) * 5,
                });
                if self.flourishes.len() > MAX_ACTIVE_POPS {
                    self.flourishes.pop_front();
                }
            }
            self.last_total = total;
        }
        let now = Instant::now();
        while self
            .flourishes
            .front()
            .is_some_and(|flourish| now.duration_since(flourish.born) >= POP_LIFETIME)
        {
            self.flourishes.pop_front();
        }

        if alpha <= 0.001 {
            if self.visible {
                // SAFETY: the HWND is live and owned by this value.
                let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
                self.visible = false;
                self.last_origin = None;
            }
            return Ok(());
        }

        // Lower-left corner of the overlay, inset by the same margin the glyphs
        // use so the number reads as belonging to the cat rather than clipped to
        // the window edge. `bounds` is in virtual-screen pixels, which is what
        // `SetWindowPos` takes.
        let origin = (
            bounds.x + HUD_MARGIN,
            bounds.y + bounds.height as i32 - HUD_HEIGHT - HUD_MARGIN,
        );
        self.paint(alpha)?;
        self.place(origin)?;
        if !self.visible {
            // SAFETY: the HWND is live and owned by this value.
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.visible = true;
        }
        Ok(())
    }

    /// Reposition without stealing focus or activation: the counter is never the
    /// foreground window, even for a moment.
    fn place(&mut self, origin: (i32, i32)) -> Result<(), OverlayError> {
        if self.last_origin == Some(origin) {
            return Ok(());
        }
        let (x, y) = origin;
        // SAFETY: the HWND is live and owned by this value.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )
        }
        .map_err(windows_error("position the keystroke counter"))?;
        self.last_origin = Some(origin);
        Ok(())
    }

    /// Draw one frame into an off-screen bitmap and hand it to the window.
    ///
    /// The bitmap is rebuilt per frame because at this size it is a few thousand
    /// pixels, and rebuilding keeps the outstanding GDI objects at zero instead
    /// of pinning a live DIB section to the session for its whole lifetime.
    fn paint(&self, alpha: f32) -> Result<(), OverlayError> {
        let mut pixels = vec![0_u8; (HUD_WIDTH * HUD_HEIGHT * 4) as usize];
        let baseline_y = HUD_HEIGHT - HUD_MARGIN - GLYPH_HEIGHT;

        let text = if self.last_total > 999_999 {
            "999999".as_bytes().to_vec()
        } else {
            self.last_total.to_string().into_bytes()
        };
        let mut cursor_x = HUD_WIDTH - HUD_MARGIN - (text.len() as i32) * GLYPH_ADVANCE;
        for digit in text {
            draw_glyph(&mut pixels, DIGIT_GLYPHS[(digit - b'0') as usize], cursor_x, baseline_y, 1.0);
            cursor_x += GLYPH_ADVANCE;
        }

        let now = Instant::now();
        let flourish_x = (HUD_WIDTH - FLOURISH_WIDTH) / 2;
        for flourish in &self.flourishes {
            let progress = (now.duration_since(flourish.born).as_secs_f32()
                / POP_LIFETIME.as_secs_f32())
            .clamp(0.0, 1.0);
            // Fade in over the first fifth and out for the rest, rising the whole
            // time, so the eye reads it as leaving rather than vanishing.
            let visibility = if progress < 0.2 {
                progress / 0.2
            } else {
                1.0 - (progress - 0.2) / 0.8
            };
            let y = baseline_y - GLYPH_HEIGHT / 2 - (progress * POP_TRAVEL as f32) as i32;
            draw_glyph(&mut pixels, PLUS_GLYPH, flourish_x + flourish.jitter, y, visibility);
            draw_glyph(
                &mut pixels,
                DIGIT_GLYPHS[1],
                flourish_x + flourish.jitter + GLYPH_ADVANCE,
                y,
                visibility,
            );
        }

        // SAFETY: the DCs and the bitmap below are created and released here, and
        // `bits` is only written to through the exact length `pixels` has.
        let result = unsafe { self.present(&pixels, alpha) };
        result
    }

    /// Copy `pixels` into a top-down 32bpp DIB and composite it onto the window.
    ///
    /// # Safety
    ///
    /// `pixels` must hold exactly `HUD_WIDTH * HUD_HEIGHT * 4` bytes, which is
    /// what every caller above guarantees by construction.
    unsafe fn present(&self, pixels: &[u8], alpha: f32) -> Result<(), OverlayError> {
        let memory_dc = unsafe { CreateCompatibleDC(None) };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: HUD_WIDTH,
                // Negative height keeps the rows top-down, which is the order the
                // glyph blit writes them in.
                biHeight: -HUD_HEIGHT,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default()],
        };
        let bitmap = unsafe {
            CreateDIBSection(
                Some(memory_dc),
                &header,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )
        };
        let bitmap = match bitmap {
            Ok(bitmap) => bitmap,
            Err(error) => {
                let _ = unsafe { DeleteDC(memory_dc) };
                return Err(windows_error("allocate the keystroke counter surface")(
                    error,
                ));
            }
        };
        if !bits.is_null() {
            let target =
                unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, pixels.len()) };
            target.copy_from_slice(pixels);
        }
        let previous = unsafe { SelectObject(memory_dc, bitmap.into()) };

        let blend = BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            // `AC_SRC_ALPHA` multiplies this constant with each pixel's own alpha,
            // so the counter can follow the overlay's fade while a flourish keeps
            // an opacity of its own.
            SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let origin = POINT { x: 0, y: 0 };
        let extent = SIZE {
            cx: HUD_WIDTH,
            cy: HUD_HEIGHT,
        };
        // A null destination point leaves the position `SetWindowPos` gave this
        // window alone, so where it sits stays that call's business.
        let updated = unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                None,
                None,
                Some(&extent),
                Some(memory_dc),
                Some(&origin),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        };
        let _ = unsafe { SelectObject(memory_dc, previous) };
        let _ = unsafe { DeleteObject(bitmap.into()) };
        let _ = unsafe { DeleteDC(memory_dc) };
        updated.map_err(windows_error("present the keystroke counter"))
    }

    /// Take the counter off the desktop and release its window.
    pub(crate) fn finish(&mut self) {
        if !self.hwnd.is_invalid() {
            // SAFETY: this HWND was created here and is destroyed exactly once.
            let _ = unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = HWND(std::ptr::null_mut());
        }
        self.visible = false;
    }
}

impl Drop for KeystrokeHud {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Blit one 5x7 glyph, outlined black then filled white.
///
/// `alpha` scales every pixel it writes, which is what makes a flourish fade.
/// Values are stored premultiplied because that is the form
/// `UpdateLayeredWindow` composites.
fn draw_glyph(pixels: &mut [u8], glyph: [u8; 7], origin_x: i32, origin_y: i32, alpha: f32) {
    for pass in 0..2 {
        let (color, spread) = if pass == 0 {
            (GLYPH_OUTLINE, 1)
        } else {
            (GLYPH_FILL, 0)
        };
        for row in 0..GLYPH_ROWS {
            for column in 0..GLYPH_COLS {
                if glyph[row as usize] & (1 << (GLYPH_COLS - 1 - column)) == 0 {
                    continue;
                }
                for y in 0..(GLYPH_SCALE + spread * 2) {
                    for x in 0..(GLYPH_SCALE + spread * 2) {
                        let pixel_x = origin_x + column * GLYPH_SCALE + x - spread;
                        let pixel_y = origin_y + row * GLYPH_SCALE + y - spread;
                        write_pixel(pixels, pixel_x, pixel_y, color, alpha);
                    }
                }
            }
        }
    }
}

/// Write one premultiplied BGRA pixel, clipped to the bitmap.
fn write_pixel(pixels: &mut [u8], x: i32, y: i32, (r, g, b): (u8, u8, u8), alpha: f32) {
    if x < 0 || y < 0 || x >= HUD_WIDTH || y >= HUD_HEIGHT {
        return;
    }
    let index = ((y * HUD_WIDTH + x) * 4) as usize;
    let coverage = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    if coverage == 0 {
        return;
    }
    let scale = f32::from(coverage) / 255.0;
    pixels[index] = (f32::from(b) * scale).round() as u8;
    pixels[index + 1] = (f32::from(g) * scale).round() as u8;
    pixels[index + 2] = (f32::from(r) * scale).round() as u8;
    pixels[index + 3] = coverage;
}
