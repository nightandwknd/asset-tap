//! Pointer position during an OS file drag.
//!
//! winit 0.30 reports file drags as `HoveredFile` / `DroppedFile` with **no
//! coordinates**, and it emits no `CursorMoved` for the duration of the drag
//! (macOS implements only `draggingEntered:`/`performDragOperation:`; the X11
//! backend says so in a comment; Wayland has no file drop at all). So
//! `Context::pointer_latest_pos()` holds whatever it held before the drag
//! started — usually `None`, because the pointer left the window to go pick
//! the file up. Upstream: <https://github.com/emilk/egui/issues/4655>.
//!
//! That makes per-zone drop targets impossible with egui alone. This module
//! asks the OS where the cursor actually is and feeds it back as a synthetic
//! `PointerMoved`, so every ordinary egui hover path — `hover_pos`,
//! `rect_contains_pointer`, `Response::hovered` — works during a drag.
//!
//! Where the OS cursor can't be queried (X11, Wayland, or a failed query) no
//! position is injected, so no zone can match and egui's pointer stays `None`
//! for the frame. `App::drop_unclaimed` reads that absence as "zones are not
//! available" and routes the drop by file type instead.

use eframe::egui;

/// How often to re-poll the cursor while a drag hovers. A drop zone's overlay
/// has to keep up with the pointer, but 60 Hz is plenty and a drag can last
/// several seconds.
const DRAG_REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Cursor position in physical pixels, in the same global screen space winit
/// reports window positions in. `None` where the platform can't say.
pub fn cursor_physical(native_pixels_per_point: f32) -> Option<egui::Pos2> {
    platform::cursor_physical(native_pixels_per_point)
}

/// Window-local position in egui points for a cursor at `physical`.
///
/// `inner_rect_min` is the window content's top-left in egui points (global
/// screen space, top-left origin) and `pixels_per_point` is
/// `Context::pixels_per_point`, so egui's zoom factor is already folded in.
fn local_pos(
    physical: egui::Pos2,
    inner_rect_min: egui::Pos2,
    pixels_per_point: f32,
) -> egui::Pos2 {
    egui::pos2(
        physical.x / pixels_per_point - inner_rect_min.x,
        physical.y / pixels_per_point - inner_rect_min.y,
    )
}

/// Feed the real cursor position into `raw_input` while files are being
/// dragged over the window. `last` carries the position across to the drop
/// frame, since the drop is delivered a frame or more after the last hover and
/// the cursor may have moved on by then.
///
/// Call from [`eframe::App::raw_input_hook`].
pub fn inject_drag_pointer(
    ctx: &egui::Context,
    raw_input: &mut egui::RawInput,
    last: &mut Option<egui::Pos2>,
) {
    let hovering = !raw_input.hovered_files.is_empty();
    let dropping = !raw_input.dropped_files.is_empty();
    if !hovering && !dropping {
        *last = None;
        return;
    }

    if hovering {
        // No further events arrive during a drag, so nothing else would drive
        // a repaint and the overlay would freeze where the drag entered.
        // Throttled like the rest of the app's self-driven repaints rather
        // than run at the display's max rate for the length of the drag.
        ctx.request_repaint_after(DRAG_REPAINT_INTERVAL);
    }

    let viewport = raw_input.viewport();
    let Some(inner_rect) = viewport.inner_rect else {
        return;
    };
    let native_pixels_per_point = viewport.native_pixels_per_point.unwrap_or(1.0);
    let pixels_per_point = ctx.zoom_factor() * native_pixels_per_point;

    let live = cursor_physical(native_pixels_per_point)
        .map(|physical| local_pos(physical, inner_rect.min, pixels_per_point));

    // On the drop frame prefer the last hovered position: the drop is what the
    // user aimed at, not wherever the cursor drifted while the event queued.
    let pos = if dropping { last.or(live) } else { live };
    let Some(pos) = pos else {
        return;
    };
    *last = Some(pos);

    // Appended last so it wins over any `PointerGone` winit emitted when the
    // cursor crossed the window edge to start the drag.
    raw_input.events.push(egui::Event::PointerMoved(pos));
}

#[cfg(target_os = "macos")]
mod platform {
    use eframe::egui;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    /// `CGEventGetLocation` is in global display points with the main
    /// display's top-left as origin — the same space winit flips window
    /// positions into — so scaling by the window's factor gives physical px.
    pub(super) fn cursor_physical(native_pixels_per_point: f32) -> Option<egui::Pos2> {
        // SAFETY: `CGEventCreate(NULL)` returns a retained event carrying the
        // current cursor location, or null. We read it, then release our
        // reference. Neither call touches memory we own.
        let point = unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event);
            point
        };
        Some(egui::pos2(
            point.x as f32 * native_pixels_per_point,
            point.y as f32 * native_pixels_per_point,
        ))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use eframe::egui;

    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut POINT) -> i32;
    }

    /// Already physical pixels on the virtual screen, which is winit's space
    /// for window positions too.
    pub(super) fn cursor_physical(_native_pixels_per_point: f32) -> Option<egui::Pos2> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `GetCursorPos` only writes the two fields of our stack POINT.
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return None;
        }
        Some(egui::pos2(point.x as f32, point.y as f32))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use eframe::egui;

    pub(super) fn cursor_physical(_native_pixels_per_point: f32) -> Option<egui::Pos2> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_pos_subtracts_window_origin_in_points() {
        // A 2x display: window content starts 100 pt from the screen's left.
        let pos = local_pos(egui::pos2(400.0, 600.0), egui::pos2(100.0, 50.0), 2.0);
        assert_eq!(pos, egui::pos2(100.0, 250.0));
    }

    #[test]
    fn local_pos_accounts_for_zoom() {
        // pixels_per_point folds in egui's zoom factor, so the same physical
        // cursor lands on a different point when the user zooms the UI.
        let unzoomed = local_pos(egui::pos2(400.0, 400.0), egui::pos2(0.0, 0.0), 2.0);
        let zoomed = local_pos(egui::pos2(400.0, 400.0), egui::pos2(0.0, 0.0), 3.0);
        assert_eq!(unzoomed, egui::pos2(200.0, 200.0));
        assert!(zoomed.x < unzoomed.x);
    }

    #[test]
    fn drag_pointer_is_not_injected_without_a_file_drag() {
        let ctx = egui::Context::default();
        let mut raw_input = egui::RawInput::default();
        let mut last = Some(egui::pos2(1.0, 2.0));
        inject_drag_pointer(&ctx, &mut raw_input, &mut last);
        assert!(raw_input.events.is_empty());
        assert_eq!(last, None, "a finished drag must not leak its position");
    }

    #[test]
    fn drop_frame_reuses_the_last_hovered_position() {
        let ctx = egui::Context::default();
        let mut raw_input = egui::RawInput {
            dropped_files: vec![egui::DroppedFile::default()],
            ..Default::default()
        };
        raw_input
            .viewports
            .entry(raw_input.viewport_id)
            .or_default();
        let viewport = raw_input.viewports.get_mut(&raw_input.viewport_id).unwrap();
        viewport.inner_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        ));
        viewport.native_pixels_per_point = Some(1.0);

        let mut last = Some(egui::pos2(42.0, 24.0));
        inject_drag_pointer(&ctx, &mut raw_input, &mut last);
        assert_eq!(
            raw_input.events,
            vec![egui::Event::PointerMoved(egui::pos2(42.0, 24.0))]
        );
    }
}
