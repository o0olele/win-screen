use crate::{capture, CapturedImage, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDecision {
    Confirm,
    Pin,
    Cancel,
    /// Finish and save the (possibly annotated) region to a file.
    Save,
    /// Finish the selection and record the selected region (no still capture).
    Record,
}

/// Configuration for an interactive selection overlay that pairs an *editable*
/// region with an external (e.g. Tauri WebView) toolbar.
///
/// The overlay freezes the screen behind it, dims everything except the
/// selection, and lets the user move / resize / re-draw the selection. Where the
/// toolbar sits, the overlay paints a COLOR_KEY "hole" — those pixels are
/// click-through, so the host's toolbar window receives clicks while the rest of
/// the overlay stays interactive.
#[cfg(windows)]
pub struct InteractiveOverlay {
    /// Toolbar size in **physical** pixels. The overlay punches a transparent
    /// hole of exactly this size and asks the host to position the toolbar there,
    /// so both sides stay aligned regardless of DPI scaling.
    pub toolbar_size: (u32, u32),
    /// Maximum toolbar height (physical px) the host may grow to when it reveals a
    /// contextual options panel. Used only to decide whether the toolbar is placed
    /// below or above the selection, so the strip stays anchored as the panel
    /// opens/closes. The actual height is driven at runtime via
    /// [`AnnotationCommand::ResizeToolbar`].
    pub toolbar_expanded_height: u32,
    /// Move the toolbar to the given screen rect and show it. Called when the
    /// selection is first committed and after every move/resize.
    pub place_toolbar: Box<dyn Fn(crate::Rect) + Send>,
    /// Resize/reposition the toolbar window *without* showing or focusing it.
    /// Called when [`AnnotationCommand::ResizeToolbar`] grows/shrinks the toolbar
    /// so it does not steal focus from in-overlay text editing.
    pub resize_toolbar: Box<dyn Fn(crate::Rect) + Send>,
    /// Temporarily hide the toolbar while a drag (move/resize/re-select) is in
    /// progress.
    pub hide_toolbar: Box<dyn Fn() + Send>,
    /// Called once with the overlay HWND (as `usize`) when the toolbar first
    /// appears, so the host can post decisions back via [`post_decision`].
    pub on_ready: Box<dyn Fn(usize) + Send>,
}

pub fn interactive_capture() -> Result<Option<CapturedImage>> {
    Ok(interactive_capture_selection()?.map(|(_, image)| image))
}

pub fn interactive_capture_selection() -> Result<Option<(crate::Rect, CapturedImage)>> {
    Ok(interactive_capture_selection_with_decision(|_| {
        SelectionDecision::Confirm
    })?
    .map(|(rect, image, _)| (rect, image)))
}

pub fn interactive_capture_selection_with_decision<F>(
    decide: F,
) -> Result<Option<(crate::Rect, CapturedImage, SelectionDecision)>>
where
    F: FnOnce(crate::Rect) -> SelectionDecision + Send + 'static,
{
    #[cfg(windows)]
    {
        let config = windows_overlay::OverlayConfig::with_decide(Box::new(decide));
        let Some((rect, decision, _doc)) = windows_overlay::run_overlay(config)? else {
            return Ok(None);
        };
        if matches!(decision, SelectionDecision::Cancel) {
            return Ok(None);
        }
        let image = capture::capture_region(rect)?;
        return Ok(Some((rect, image, decision)));
    }

    #[cfg(not(windows))]
    {
        let _ = decide;
        Err(crate::WinScreenError::UnsupportedPlatform)
    }
}

/// Run an interactive selection with an *editable* region and an external
/// toolbar (move / resize / re-select). See [`InteractiveOverlay`].
#[cfg(windows)]
pub fn interactive_capture_selection_with_overlay(
    overlay: InteractiveOverlay,
) -> Result<Option<(crate::Rect, CapturedImage, SelectionDecision)>> {
    let config = windows_overlay::OverlayConfig::interactive(overlay);
    let Some((rect, decision, doc)) = windows_overlay::run_overlay(config)? else {
        return Ok(None);
    };
    if matches!(decision, SelectionDecision::Cancel) {
        return Ok(None);
    }
    let image = capture::capture_region(rect)?;
    // Annotations are stored in screen-absolute coordinates; map them into the
    // captured region's local space and composite onto the image.
    let image = if doc.is_empty() {
        image
    } else {
        doc.render_offset(&image, -rect.x, -rect.y)?
    };
    Ok(Some((rect, image, decision)))
}

/// Post a toolbar decision back to a running overlay, identified by the HWND
/// delivered through [`InteractiveOverlay::on_ready`].
#[cfg(windows)]
pub fn post_decision(hwnd: usize, decision: SelectionDecision) {
    windows_overlay::post_decision(hwnd, decision);
}

/// Post an annotation command (select tool / color / width, undo-redo-clear, or
/// finish via confirm/pin/save/cancel) to a running interactive overlay. The
/// overlay does the drawing; the host only sends button presses. Identified by the
/// HWND delivered through [`InteractiveOverlay::on_ready`].
#[cfg(windows)]
pub fn post_overlay_command(hwnd: usize, command: crate::AnnotationCommand) {
    windows_overlay::post_overlay_command(hwnd, command);
}

#[cfg(windows)]
mod windows_overlay {
    use super::SelectionDecision;
    use crate::annotate::{
        arrow_head_wings, AnnotationCommand, AnnotationDocument, AnnotationShape, AnnotationTool,
        Color, Point,
    };
    use crate::{platform, Rect, Result, WinScreenError};
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        AlphaBlend, BeginPaint, BitBlt, CombineRgn, CreateCompatibleBitmap, CreateCompatibleDC,
        CreatePen, CreateRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, Ellipse,
        EndPaint, FillRect, GetStockObject, GetWindowDC, IntersectClipRect, InvalidateRect, LineTo,
        MoveToEx, Rectangle, ReleaseDC, SelectClipRgn, SelectObject, SetBkColor, SetBkMode,
        SetTextColor, SetWindowRgn, BLACK_BRUSH, BLENDFUNCTION, CAPTUREBLT, DT_CENTER,
        DT_SINGLELINE, DT_VCENTER, HBITMAP, HBRUSH, HDC, HGDIOBJ, HRGN, NULL_BRUSH, PAINTSTRUCT,
        PS_SOLID, RGN_DIFF, SRCCOPY, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
        GetCursorPos, GetDesktopWindow, GetMessageW, GetWindowLongPtrW, GetWindowRect, IsIconic,
        IsWindowVisible, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW, SetCursor,
        SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW,
        CS_VREDRAW, GWLP_USERDATA, HCURSOR, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
        IDC_SIZENWSE, IDC_SIZEWE, LWA_COLORKEY, MSG, SW_SHOW, WM_CHAR, WM_DESTROY, WM_ERASEBKGND,
        WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN,
        WM_SETCURSOR, WM_USER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "WinScreenRegionOverlay";
    // Rare color used as the transparency key — pixels of this exact color become
    // both invisible and click-through (so the toolbar hole reaches the toolbar).
    const COLOR_KEY: COLORREF = COLORREF(0x00010203);
    // Custom posted message: wparam encodes decision (0=Confirm, 1=Pin, 2=Cancel, 3=Save).
    const WM_DECIDE_DONE: u32 = WM_USER + 1;
    // Custom posted message carrying an AnnotationCommand from the host toolbar.
    // wparam = kind, lparam = payload (tool index / packed RGBA / stroke width).
    const WM_OVERLAY_CMD: u32 = WM_USER + 2;
    // How much the screen is dimmed outside the selection (0=black .. 255=clear).
    const DIM_ALPHA: u8 = 140;
    // Gap between the selection and the toolbar, and resize-handle sizing.
    const TOOLBAR_GAP: i32 = 8;
    const HANDLE_HALF: i32 = 4;
    const GRAB_MARGIN: i32 = 8;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        Selecting,
        Editing,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Handle {
        TopLeft,
        Top,
        TopRight,
        Right,
        BottomRight,
        Bottom,
        BottomLeft,
        Left,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum HitZone {
        Handle(Handle),
        Inside,
        Outside,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Drag {
        None,
        NewSelection,
        Move,
        Resize(Handle),
    }

    pub(super) struct OverlayConfig {
        interactive: bool,
        decide: Option<Box<dyn FnOnce(Rect) -> SelectionDecision + Send>>,
        toolbar_size: (u32, u32),
        toolbar_expanded_height: u32,
        place_toolbar: Option<Box<dyn Fn(Rect) + Send>>,
        resize_toolbar: Option<Box<dyn Fn(Rect) + Send>>,
        hide_toolbar: Option<Box<dyn Fn() + Send>>,
        on_ready: Option<Box<dyn Fn(usize) + Send>>,
    }

    impl OverlayConfig {
        pub(super) fn with_decide(
            decide: Box<dyn FnOnce(Rect) -> SelectionDecision + Send>,
        ) -> Self {
            Self {
                interactive: false,
                decide: Some(decide),
                toolbar_size: (0, 0),
                toolbar_expanded_height: 0,
                place_toolbar: None,
                resize_toolbar: None,
                hide_toolbar: None,
                on_ready: None,
            }
        }

        pub(super) fn interactive(o: super::InteractiveOverlay) -> Self {
            Self {
                interactive: true,
                decide: None,
                toolbar_size: o.toolbar_size,
                toolbar_expanded_height: o.toolbar_expanded_height,
                place_toolbar: Some(o.place_toolbar),
                resize_toolbar: Some(o.resize_toolbar),
                hide_toolbar: Some(o.hide_toolbar),
                on_ready: Some(o.on_ready),
            }
        }
    }

    struct OverlayState {
        virtual_rect: Rect,
        // Frozen screenshot used as the overlay background.
        shot_dc: HDC,
        shot_bmp: HBITMAP,
        shot_old: HGDIOBJ,
        // Selection / interaction.
        phase: Phase,
        drag: Drag,
        start: Option<POINT>, // new-selection drag start (screen coords)
        current: POINT,       // new-selection drag current (screen coords)
        drag_origin: POINT,   // move/resize start cursor (screen coords)
        orig_rect: Rect,      // selection at move/resize start
        selection: Option<Rect>,
        hover_win: Option<Rect>, // window auto-highlight before the first drag
        hover_hit: HitZone,
        // Annotation (drawn directly on the overlay, screen-absolute coords).
        doc: AnnotationDocument,
        active_tool: Option<AnnotationTool>, // None = selection mode, Some = annotating
        color: Color,
        stroke_width: u32,
        font_size: u32, // text tool font size (physical px)
        draw_start: Option<Point>, // shape drag start (screen coords)
        draw_current: Point,       // shape drag current (screen coords)
        brush_points: Vec<Point>,  // freehand points (screen coords)
        number: u32,
        text_origin: Option<Point>, // active text caret (screen coords)
        text_buffer: String,
        // Outcome.
        result: Option<Rect>,
        decision: SelectionDecision,
        canceled: bool,
        // Config.
        interactive: bool,
        decide: Option<Box<dyn FnOnce(Rect) -> SelectionDecision + Send>>,
        toolbar_size: (u32, u32),
        // Collapsed (strip-only) and fully-expanded toolbar heights (physical px).
        // `strip_height` is the initial height; the live height lives in
        // `toolbar_size.1` and is changed by ResizeToolbar.
        strip_height: u32,
        expanded_height: u32,
        place_toolbar: Option<Box<dyn Fn(Rect) + Send>>,
        resize_toolbar: Option<Box<dyn Fn(Rect) + Send>>,
        hide_toolbar: Option<Box<dyn Fn() + Send>>,
        on_ready: Option<Box<dyn Fn(usize) + Send>>,
        ready_sent: bool,
        // Cursors.
        cur_cross: HCURSOR,
        cur_size_all: HCURSOR,
        cur_nwse: HCURSOR,
        cur_nesw: HCURSOR,
        cur_we: HCURSOR,
        cur_ns: HCURSOR,
    }

    pub(super) fn run_overlay(
        config: OverlayConfig,
    ) -> Result<Option<(Rect, SelectionDecision, AnnotationDocument)>> {
        platform::set_process_dpi_aware().ok();
        let virtual_rect = platform::virtual_screen_rect()?;
        let class_name = wide(CLASS_NAME);
        let hinstance = unsafe { GetModuleHandleW(None) }?;

        let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }?;
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&wnd_class);
        }

        let (shot_dc, shot_bmp, shot_old) = unsafe { capture_screen(virtual_rect)? };

        let load = |id| unsafe { LoadCursorW(None, id) }.unwrap_or(cursor);
        let mut state = OverlayState {
            virtual_rect,
            shot_dc,
            shot_bmp,
            shot_old,
            phase: Phase::Selecting,
            drag: Drag::None,
            start: None,
            current: POINT {
                x: virtual_rect.x,
                y: virtual_rect.y,
            },
            drag_origin: POINT::default(),
            orig_rect: virtual_rect,
            selection: None,
            hover_win: None,
            hover_hit: HitZone::Outside,
            doc: AnnotationDocument::new(),
            active_tool: None,
            color: Color::RED,
            stroke_width: 3,
            font_size: 22,
            draw_start: None,
            draw_current: Point { x: 0, y: 0 },
            brush_points: Vec::new(),
            number: 1,
            text_origin: None,
            text_buffer: String::new(),
            result: None,
            decision: SelectionDecision::Confirm,
            canceled: false,
            interactive: config.interactive,
            decide: config.decide,
            toolbar_size: config.toolbar_size,
            strip_height: config.toolbar_size.1,
            expanded_height: config.toolbar_expanded_height.max(config.toolbar_size.1),
            place_toolbar: config.place_toolbar,
            resize_toolbar: config.resize_toolbar,
            hide_toolbar: config.hide_toolbar,
            on_ready: config.on_ready,
            ready_sent: false,
            cur_cross: cursor,
            cur_size_all: load(IDC_SIZEALL),
            cur_nwse: load(IDC_SIZENWSE),
            cur_nesw: load(IDC_SIZENESW),
            cur_we: load(IDC_SIZEWE),
            cur_ns: load(IDC_SIZENS),
        };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(wide("win-screen overlay").as_ptr()),
                WS_POPUP | WS_VISIBLE,
                virtual_rect.x,
                virtual_rect.y,
                virtual_rect.width as i32,
                virtual_rect.height as i32,
                None,
                None,
                hinstance,
                None,
            )
        }?;

        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut state as *mut OverlayState) as isize,
            );
            // Only COLOR_KEY pixels are transparent; everything else is opaque.
            SetLayeredWindowAttributes(hwnd, COLOR_KEY, 0, LWA_COLORKEY)?;
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        unsafe {
            SelectObject(state.shot_dc, state.shot_old);
            let _ = DeleteObject(HGDIOBJ(state.shot_bmp.0));
            let _ = DeleteDC(state.shot_dc);
        }

        if state.canceled {
            Ok(None)
        } else {
            let doc = std::mem::take(&mut state.doc);
            Ok(state.result.map(|rect| (rect, state.decision, doc)))
        }
    }

    pub(super) fn post_decision(hwnd: usize, decision: SelectionDecision) {
        let code: usize = match decision {
            SelectionDecision::Confirm => 0,
            SelectionDecision::Pin => 1,
            SelectionDecision::Cancel => 2,
            SelectionDecision::Save => 3,
            SelectionDecision::Record => 4,
        };
        unsafe {
            let _ = PostMessageW(
                HWND(hwnd as *mut c_void),
                WM_DECIDE_DONE,
                WPARAM(code),
                LPARAM(0),
            );
        }
    }

    pub(super) fn post_overlay_command(hwnd: usize, command: AnnotationCommand) {
        let (kind, payload): (usize, isize) = match command {
            AnnotationCommand::SetTool(tool) => (0, tool_index(tool) as isize),
            AnnotationCommand::SetColor(color) => (1, pack_rgba(color) as isize),
            AnnotationCommand::SetStrokeWidth(width) => (2, width as isize),
            AnnotationCommand::Undo => (3, 0),
            AnnotationCommand::Redo => (4, 0),
            AnnotationCommand::Clear => (5, 0),
            AnnotationCommand::Confirm => (6, 0),
            AnnotationCommand::Pin => (7, 0),
            AnnotationCommand::Save => (8, 0),
            AnnotationCommand::Cancel => (9, 0),
            AnnotationCommand::DeselectTool => (10, 0),
            AnnotationCommand::SetFontSize(size) => (11, size as isize),
            AnnotationCommand::ResizeToolbar(height) => (12, height as isize),
            AnnotationCommand::Record => (13, 0),
        };
        unsafe {
            let _ = PostMessageW(
                HWND(hwnd as *mut c_void),
                WM_OVERLAY_CMD,
                WPARAM(kind),
                LPARAM(payload),
            );
        }
    }

    fn tool_index(tool: AnnotationTool) -> usize {
        match tool {
            AnnotationTool::Rectangle => 0,
            AnnotationTool::Ellipse => 1,
            AnnotationTool::Arrow => 2,
            AnnotationTool::Line => 3,
            AnnotationTool::Brush => 4,
            AnnotationTool::Text => 5,
            AnnotationTool::Mosaic => 6,
            AnnotationTool::Number => 7,
        }
    }

    fn tool_from_index(idx: usize) -> Option<AnnotationTool> {
        Some(match idx {
            0 => AnnotationTool::Rectangle,
            1 => AnnotationTool::Ellipse,
            2 => AnnotationTool::Arrow,
            3 => AnnotationTool::Line,
            4 => AnnotationTool::Brush,
            5 => AnnotationTool::Text,
            6 => AnnotationTool::Mosaic,
            7 => AnnotationTool::Number,
            _ => return None,
        })
    }

    fn pack_rgba(color: Color) -> i32 {
        ((color.r as i32) << 24)
            | ((color.g as i32) << 16)
            | ((color.b as i32) << 8)
            | (color.a as i32)
    }

    fn unpack_rgba(value: i32) -> Color {
        Color {
            r: ((value >> 24) & 0xff) as u8,
            g: ((value >> 16) & 0xff) as u8,
            b: ((value >> 8) & 0xff) as u8,
            a: (value & 0xff) as u8,
        }
    }

    fn colorref(color: Color) -> COLORREF {
        COLORREF(color.r as u32 | ((color.g as u32) << 8) | ((color.b as u32) << 16))
    }

    /// Clamp a screen point into the selection rect, so drawing stays inside the
    /// captured region.
    fn clamp_to_sel(p: POINT, sel: Rect) -> Point {
        let l = sel.x;
        let t = sel.y;
        let r = sel.x + sel.width as i32;
        let b = sel.y + sel.height as i32;
        Point {
            x: p.x.clamp(l, r),
            y: p.y.clamp(t, b),
        }
    }

    unsafe fn capture_screen(vr: Rect) -> Result<(HDC, HBITMAP, HGDIOBJ)> {
        let desktop = GetDesktopWindow();
        let screen_dc = GetWindowDC(desktop);
        if screen_dc.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "overlay GetWindowDC",
            });
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmp = CreateCompatibleBitmap(screen_dc, vr.width as i32, vr.height as i32);
        let old = SelectObject(mem_dc, HGDIOBJ(bmp.0));
        let _ = BitBlt(
            mem_dc,
            0,
            0,
            vr.width as i32,
            vr.height as i32,
            screen_dc,
            vr.x,
            vr.y,
            SRCCOPY | CAPTUREBLT,
        );
        ReleaseDC(desktop, screen_dc);
        Ok((mem_dc, bmp, old))
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState;

        match msg {
            WM_LBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    let point = cursor_point();
                    // Annotation mode: a tool is active → draw on the selection.
                    if state.interactive {
                        if let (Some(tool), Some(sel)) = (state.active_tool, state.selection) {
                            begin_annotation(hwnd, state, tool, sel, point);
                            return LRESULT(0);
                        }
                    }
                    if state.interactive && state.phase == Phase::Editing {
                        if let Some(sel) = state.selection {
                            match hit_test(sel, point) {
                                HitZone::Handle(h) => {
                                    state.drag = Drag::Resize(h);
                                    state.drag_origin = point;
                                    state.orig_rect = sel;
                                }
                                HitZone::Inside => {
                                    state.drag = Drag::Move;
                                    state.drag_origin = point;
                                    state.orig_rect = sel;
                                }
                                HitZone::Outside => {
                                    // Re-selecting a new region clears any annotations.
                                    state.doc.clear();
                                    state.drag = Drag::NewSelection;
                                    state.start = Some(point);
                                    state.current = point;
                                }
                            }
                            SetCapture(hwnd);
                            hide_toolbar(state);
                            clear_toolbar_hole(hwnd);
                            let _ = InvalidateRect(hwnd, None, false);
                            return LRESULT(0);
                        }
                    }
                    // First selection (or non-interactive path).
                    if state.result.is_some() {
                        return LRESULT(0);
                    }
                    state.drag = Drag::NewSelection;
                    state.start = Some(point);
                    state.current = point;
                    SetCapture(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state_ptr.as_mut() {
                    let point = cursor_point();
                    // Annotation drag in progress.
                    if state.draw_start.is_some() {
                        if let Some(sel) = state.selection {
                            let p = clamp_to_sel(point, sel);
                            state.draw_current = p;
                            if matches!(state.active_tool, Some(AnnotationTool::Brush)) {
                                state.brush_points.push(p);
                            }
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        return LRESULT(0);
                    }
                    match state.drag {
                        Drag::NewSelection => {
                            state.current = point;
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        Drag::Move => {
                            state.selection =
                                Some(apply_move(state.orig_rect, state.drag_origin, point, state.virtual_rect));
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        Drag::Resize(h) => {
                            state.selection =
                                Some(apply_resize(state.orig_rect, h, point, state.virtual_rect));
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        Drag::None => {
                            if state.phase == Phase::Selecting && state.selection.is_none() {
                                let next = window_rect_from_point(hwnd, point);
                                if state.hover_win != next {
                                    state.hover_win = next;
                                    let _ = InvalidateRect(hwnd, None, false);
                                }
                            } else if state.interactive && state.phase == Phase::Editing {
                                if let Some(sel) = state.selection {
                                    state.hover_hit = hit_test(sel, point);
                                }
                            }
                        }
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                if let Some(state) = state_ptr.as_mut() {
                    let point = cursor_point();
                    // Finish an annotation drag.
                    if state.draw_start.is_some() {
                        if let Some(sel) = state.selection {
                            state.draw_current = clamp_to_sel(point, sel);
                        }
                        if let Some(shape) = overlay_preview_shape(state) {
                            state.doc.push(shape);
                        }
                        state.draw_start = None;
                        state.brush_points.clear();
                        let _ = ReleaseCapture();
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    match state.drag {
                        Drag::Move | Drag::Resize(_) => {
                            state.drag = Drag::None;
                            let _ = ReleaseCapture();
                            if let Some(sel) = state.selection {
                                let tr = toolbar_rect(state, sel);
                                if let Some(place) = &state.place_toolbar {
                                    place(tr);
                                }
                            }
                            set_toolbar_hole(hwnd, state);
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        Drag::NewSelection => {
                            state.drag = Drag::None;
                            state.current = point;
                            let rect = state
                                .start
                                .and_then(|s| selection_rect_from(s, point))
                                .or(state.hover_win);
                            state.start = None;
                            state.hover_win = None;
                            let _ = ReleaseCapture();
                            if let Some(rect) = rect {
                                if state.interactive {
                                    state.selection = Some(rect);
                                    state.phase = Phase::Editing;
                                    if !state.ready_sent {
                                        state.ready_sent = true;
                                        if let Some(f) = &state.on_ready {
                                            f(hwnd.0 as usize);
                                        }
                                    }
                                    let tr = toolbar_rect(state, rect);
                                    if let Some(place) = &state.place_toolbar {
                                        place(tr);
                                    }
                                    set_toolbar_hole(hwnd, state);
                                } else {
                                    state.selection = Some(rect);
                                    state.result = Some(rect);
                                    if let Some(decide_fn) = state.decide.take() {
                                        spawn_decide(hwnd, decide_fn, rect);
                                    }
                                }
                            }
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        Drag::None => {
                            let _ = ReleaseCapture();
                        }
                    }
                    return LRESULT(0);
                }
            }
            WM_SETCURSOR => {
                if let Some(state) = state_ptr.as_ref() {
                    if state.active_tool.is_some() {
                        SetCursor(state.cur_cross);
                        return LRESULT(1);
                    }
                    let zone = match state.drag {
                        Drag::Resize(h) => HitZone::Handle(h),
                        Drag::Move => HitZone::Inside,
                        Drag::NewSelection => HitZone::Outside,
                        Drag::None => {
                            if state.interactive && state.phase == Phase::Editing {
                                match state.selection {
                                    Some(sel) => hit_test(sel, cursor_point()),
                                    None => HitZone::Outside,
                                }
                            } else {
                                HitZone::Outside
                            }
                        }
                    };
                    SetCursor(cursor_for(state, zone));
                    return LRESULT(1);
                }
            }
            WM_RBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    state.canceled = true;
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
            }
            WM_CHAR => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.text_origin.is_some() {
                        match char::from_u32(wparam.0 as u32) {
                            Some('\u{8}') => {
                                state.text_buffer.pop();
                            }
                            Some(ch) if !ch.is_control() => state.text_buffer.push(ch),
                            _ => {}
                        }
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                }
            }
            WM_KEYDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    // While typing, ESC cancels just the text and ENTER commits it.
                    if state.text_origin.is_some() {
                        if wparam.0 == VK_ESCAPE.0 as usize {
                            state.text_origin = None;
                            state.text_buffer.clear();
                            let _ = InvalidateRect(hwnd, None, false);
                            return LRESULT(0);
                        }
                        if wparam.0 == VK_RETURN.0 as usize {
                            commit_text(state);
                            let _ = InvalidateRect(hwnd, None, false);
                            return LRESULT(0);
                        }
                    }
                    if wparam.0 == VK_ESCAPE.0 as usize {
                        state.canceled = true;
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    if wparam.0 == VK_RETURN.0 as usize {
                        if state.interactive {
                            if state.phase == Phase::Editing && state.selection.is_some() {
                                let _ = PostMessageW(hwnd, WM_DECIDE_DONE, WPARAM(0), LPARAM(0));
                            }
                        } else if state.result.is_none() {
                            let rect = state
                                .start
                                .and_then(|s| selection_rect_from(s, state.current))
                                .or(state.selection)
                                .or(state.hover_win);
                            if let Some(rect) = rect {
                                state.selection = Some(rect);
                                state.result = Some(rect);
                                if let Some(decide_fn) = state.decide.take() {
                                    spawn_decide(hwnd, decide_fn, rect);
                                }
                            }
                        }
                        return LRESULT(0);
                    }
                }
            }
            WM_DECIDE_DONE => {
                if let Some(state) = state_ptr.as_mut() {
                    state.decision = match wparam.0 {
                        1 => SelectionDecision::Pin,
                        2 => SelectionDecision::Cancel,
                        3 => SelectionDecision::Save,
                        4 => SelectionDecision::Record,
                        _ => SelectionDecision::Confirm,
                    };
                    if matches!(state.decision, SelectionDecision::Cancel) {
                        state.canceled = true;
                    }
                    if state.result.is_none() {
                        state.result = state.selection;
                    }
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_OVERLAY_CMD => {
                if let Some(state) = state_ptr.as_mut() {
                    let finish = |state: &mut OverlayState, decision: SelectionDecision| {
                        state.decision = decision;
                        if state.result.is_none() {
                            state.result = state.selection;
                        }
                    };
                    match wparam.0 {
                        0 => {
                            if let Some(tool) = tool_from_index(lparam.0 as usize) {
                                state.active_tool = Some(tool);
                            }
                        }
                        10 => {
                            commit_text(state);
                            state.active_tool = None;
                        }
                        1 => state.color = unpack_rgba(lparam.0 as i32),
                        2 => state.stroke_width = (lparam.0 as u32).max(1),
                        11 => state.font_size = (lparam.0 as u32).max(8),
                        12 => {
                            // Resize the toolbar window width to fit the host's
                            // content (the frontend reports its measured width), then
                            // re-anchor it at the selection and re-cut the matching
                            // overlay hole so window, bar, and hole all coincide.
                            state.toolbar_size.0 = (lparam.0 as u32).max(1);
                            if let Some(sel) = state.selection {
                                let tr = toolbar_rect(state, sel);
                                if let Some(resize) = &state.resize_toolbar {
                                    resize(tr);
                                }
                                set_toolbar_hole(hwnd, state);
                            }
                        }
                        3 => {
                            state.doc.undo();
                        }
                        4 => {
                            state.doc.redo();
                        }
                        5 => state.doc.clear(),
                        6 => {
                            commit_text(state);
                            finish(state, SelectionDecision::Confirm);
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        7 => {
                            commit_text(state);
                            finish(state, SelectionDecision::Pin);
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        8 => {
                            commit_text(state);
                            finish(state, SelectionDecision::Save);
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        13 => {
                            commit_text(state);
                            finish(state, SelectionDecision::Record);
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        9 => {
                            state.canceled = true;
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_PAINT => {
                if let Some(state) = state_ptr.as_ref() {
                    paint(hwnd, state);
                    return LRESULT(0);
                }
            }
            WM_ERASEBKGND => {
                return LRESULT(1);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn hide_toolbar(state: &OverlayState) {
        if let Some(f) = &state.hide_toolbar {
            f();
        }
    }

    fn cursor_for(state: &OverlayState, zone: HitZone) -> HCURSOR {
        match zone {
            HitZone::Handle(Handle::TopLeft) | HitZone::Handle(Handle::BottomRight) => state.cur_nwse,
            HitZone::Handle(Handle::TopRight) | HitZone::Handle(Handle::BottomLeft) => state.cur_nesw,
            HitZone::Handle(Handle::Top) | HitZone::Handle(Handle::Bottom) => state.cur_ns,
            HitZone::Handle(Handle::Left) | HitZone::Handle(Handle::Right) => state.cur_we,
            HitZone::Inside => state.cur_size_all,
            HitZone::Outside => state.cur_cross,
        }
    }

    fn hit_test(sel: Rect, p: POINT) -> HitZone {
        let l = sel.x;
        let t = sel.y;
        let r = sel.x + sel.width as i32;
        let b = sel.y + sel.height as i32;
        let cx = (l + r) / 2;
        let cy = (t + b) / 2;
        let near = |a: i32, target: i32| (a - target).abs() <= GRAB_MARGIN;

        if near(p.x, l) && near(p.y, t) {
            return HitZone::Handle(Handle::TopLeft);
        }
        if near(p.x, r) && near(p.y, t) {
            return HitZone::Handle(Handle::TopRight);
        }
        if near(p.x, l) && near(p.y, b) {
            return HitZone::Handle(Handle::BottomLeft);
        }
        if near(p.x, r) && near(p.y, b) {
            return HitZone::Handle(Handle::BottomRight);
        }
        if near(p.x, cx) && near(p.y, t) {
            return HitZone::Handle(Handle::Top);
        }
        if near(p.x, cx) && near(p.y, b) {
            return HitZone::Handle(Handle::Bottom);
        }
        if near(p.x, l) && near(p.y, cy) {
            return HitZone::Handle(Handle::Left);
        }
        if near(p.x, r) && near(p.y, cy) {
            return HitZone::Handle(Handle::Right);
        }
        if p.x >= l && p.x <= r && p.y >= t && p.y <= b {
            return HitZone::Inside;
        }
        HitZone::Outside
    }

    fn apply_move(orig: Rect, origin: POINT, p: POINT, vr: Rect) -> Rect {
        let dx = p.x - origin.x;
        let dy = p.y - origin.y;
        let w = orig.width as i32;
        let h = orig.height as i32;
        let max_x = (vr.x + vr.width as i32 - w).max(vr.x);
        let max_y = (vr.y + vr.height as i32 - h).max(vr.y);
        let x = (orig.x + dx).clamp(vr.x, max_x);
        let y = (orig.y + dy).clamp(vr.y, max_y);
        Rect {
            x,
            y,
            width: orig.width,
            height: orig.height,
        }
    }

    fn apply_resize(orig: Rect, h: Handle, p: POINT, vr: Rect) -> Rect {
        let mut l = orig.x;
        let mut t = orig.y;
        let mut r = orig.x + orig.width as i32;
        let mut b = orig.y + orig.height as i32;
        match h {
            Handle::TopLeft => {
                l = p.x;
                t = p.y;
            }
            Handle::Top => t = p.y,
            Handle::TopRight => {
                r = p.x;
                t = p.y;
            }
            Handle::Right => r = p.x,
            Handle::BottomRight => {
                r = p.x;
                b = p.y;
            }
            Handle::Bottom => b = p.y,
            Handle::BottomLeft => {
                l = p.x;
                b = p.y;
            }
            Handle::Left => l = p.x,
        }
        let min_x = vr.x;
        let max_x = vr.x + vr.width as i32;
        let min_y = vr.y;
        let max_y = vr.y + vr.height as i32;
        l = l.clamp(min_x, max_x);
        r = r.clamp(min_x, max_x);
        t = t.clamp(min_y, max_y);
        b = b.clamp(min_y, max_y);
        let x0 = l.min(r);
        let x1 = l.max(r);
        let y0 = t.min(b);
        let y1 = t.max(b);
        Rect {
            x: x0,
            y: y0,
            width: (x1 - x0).max(1) as u32,
            height: (y1 - y0).max(1) as u32,
        }
    }

    /// Where to place the toolbar relative to the selection: below if the fully
    /// expanded toolbar fits, otherwise with the strip just above the selection;
    /// clamped horizontally to the virtual screen.
    ///
    /// The below/above decision uses `expanded_height` (not the current height) so
    /// the strip's top stays anchored as the options panel opens and closes — only
    /// the height changes, never `y`.
    fn toolbar_rect(state: &OverlayState, sel: Rect) -> Rect {
        let (tw, th) = state.toolbar_size;
        // The window can never be wider than the screen; excess content scrolls
        // horizontally inside the webview.
        let tw = tw.min(state.virtual_rect.width);
        let vr = state.virtual_rect;
        let screen_right = vr.x + vr.width as i32;
        let screen_bottom = vr.y + vr.height as i32;

        let mut x = sel.x;
        if x + tw as i32 > screen_right {
            x = screen_right - tw as i32;
        }
        if x < vr.x {
            x = vr.x;
        }

        // Reserve space for the fully expanded panel when deciding below vs above,
        // so the strip does not jump when the panel toggles.
        let expanded = state.expanded_height.max(th) as i32;
        let y = if sel.y + sel.height as i32 + TOOLBAR_GAP + expanded <= screen_bottom {
            // Fits below; the panel grows downward into the free margin.
            sel.y + sel.height as i32 + TOOLBAR_GAP
        } else {
            // Not enough room below: anchor the strip just above the selection and
            // let the panel grow downward over it. Only happens near the bottom edge.
            (sel.y - TOOLBAR_GAP - state.strip_height as i32).max(vr.y)
        };

        Rect {
            x,
            y,
            width: tw,
            height: th,
        }
    }

    /// Carve the toolbar rectangle out of the overlay window so that area is no longer
    /// part of the window: all mouse input there falls straight through to the floating
    /// toolbar beneath, regardless of z-order, color-key, or DPI. Window-local (client)
    /// coordinates, i.e. screen coords minus the virtual-rect origin.
    unsafe fn set_toolbar_hole(hwnd: HWND, state: &OverlayState) {
        let Some(sel) = state.selection else {
            return;
        };
        let vr = state.virtual_rect;
        let tr = toolbar_rect(state, sel);
        let full = CreateRectRgn(0, 0, vr.width as i32, vr.height as i32);
        let hole = CreateRectRgn(
            tr.x - vr.x,
            tr.y - vr.y,
            tr.x - vr.x + tr.width as i32,
            tr.y - vr.y + tr.height as i32,
        );
        CombineRgn(full, full, hole, RGN_DIFF);
        let _ = DeleteObject(HGDIOBJ(hole.0));
        // The window takes ownership of `full`; do not delete it here.
        SetWindowRgn(hwnd, full, true);
    }

    /// Restore the overlay to a solid full-screen window (no hole) — used while the
    /// toolbar is hidden (during a selection drag/move/resize).
    unsafe fn clear_toolbar_hole(hwnd: HWND) {
        SetWindowRgn(hwnd, HRGN::default(), true);
    }

    fn active_rect(state: &OverlayState) -> Option<Rect> {
        if matches!(state.drag, Drag::NewSelection) {
            if let Some(s) = state.start {
                if let Some(rect) = selection_rect_from(s, state.current) {
                    return Some(rect);
                }
            }
        }
        if let Some(sel) = state.selection {
            return Some(sel);
        }
        if state.phase == Phase::Selecting {
            if let Some(s) = state.start {
                if let Some(rect) = selection_rect_from(s, state.current) {
                    return Some(rect);
                }
            }
            return state.hover_win;
        }
        None
    }

    unsafe fn begin_annotation(
        hwnd: HWND,
        state: &mut OverlayState,
        tool: AnnotationTool,
        sel: Rect,
        cursor: POINT,
    ) {
        let p = clamp_to_sel(cursor, sel);
        match tool {
            AnnotationTool::Text => {
                commit_text(state);
                state.text_origin = Some(p);
                state.text_buffer.clear();
            }
            AnnotationTool::Number => {
                state.doc.push(AnnotationShape::Number {
                    center: p,
                    value: state.number,
                    fill: state.color,
                    text: Color::WHITE,
                });
                state.number += 1;
            }
            _ => {
                state.draw_start = Some(p);
                state.draw_current = p;
                state.brush_points.clear();
                state.brush_points.push(p);
                SetCapture(hwnd);
            }
        }
        let _ = InvalidateRect(hwnd, None, false);
    }

    fn commit_text(state: &mut OverlayState) {
        let Some(origin) = state.text_origin.take() else {
            return;
        };
        if !state.text_buffer.trim().is_empty() {
            state.doc.push(AnnotationShape::Text {
                origin,
                text: state.text_buffer.clone(),
                color: state.color,
                font_size: state.font_size,
            });
        }
        state.text_buffer.clear();
    }

    fn norm_rect(a: Point, b: Point) -> Option<Rect> {
        let left = a.x.min(b.x);
        let top = a.y.min(b.y);
        let right = a.x.max(b.x);
        let bottom = a.y.max(b.y);
        let width = u32::try_from(right - left).ok()?;
        let height = u32::try_from(bottom - top).ok()?;
        if width < 2 || height < 2 {
            return None;
        }
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }

    fn overlay_preview_shape(state: &OverlayState) -> Option<AnnotationShape> {
        let start = state.draw_start?;
        let tool = state.active_tool?;
        let current = state.draw_current;
        match tool {
            AnnotationTool::Rectangle => Some(AnnotationShape::Rectangle {
                rect: norm_rect(start, current)?,
                stroke: state.color,
                stroke_width: state.stroke_width,
            }),
            AnnotationTool::Ellipse => Some(AnnotationShape::Ellipse {
                rect: norm_rect(start, current)?,
                stroke: state.color,
                stroke_width: state.stroke_width,
            }),
            AnnotationTool::Arrow => Some(AnnotationShape::Arrow {
                start,
                end: current,
                stroke: state.color,
                stroke_width: state.stroke_width,
            }),
            AnnotationTool::Line => Some(AnnotationShape::Line {
                start,
                end: current,
                stroke: state.color,
                stroke_width: state.stroke_width,
            }),
            AnnotationTool::Brush => Some(AnnotationShape::Brush {
                points: state.brush_points.clone(),
                stroke: state.color,
                stroke_width: state.stroke_width.max(2),
            }),
            AnnotationTool::Mosaic => Some(AnnotationShape::Mosaic {
                rect: norm_rect(start, current)?,
                block_size: 12,
            }),
            AnnotationTool::Text | AnnotationTool::Number => None,
        }
    }

    fn lp(vr: Rect, p: Point) -> POINT {
        POINT {
            x: p.x - vr.x,
            y: p.y - vr.y,
        }
    }

    fn lr(vr: Rect, r: Rect) -> RECT {
        let left = r.x - vr.x;
        let top = r.y - vr.y;
        RECT {
            left,
            top,
            right: left + r.width as i32,
            bottom: top + r.height as i32,
        }
    }

    /// Draw a single shape onto the overlay (1:1, screen→client local coords). This
    /// is a live preview; the final pixels are composited by `AnnotationDocument`.
    unsafe fn draw_shape_overlay(hdc: HDC, vr: Rect, shape: &AnnotationShape) {
        match shape {
            AnnotationShape::Rectangle {
                rect,
                stroke,
                stroke_width,
            } => {
                let rect = lr(vr, *rect);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                let _ = Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Ellipse {
                rect,
                stroke,
                stroke_width,
            } => {
                let rect = lr(vr, *rect);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                let _ = Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Line {
                start,
                end,
                stroke,
                stroke_width,
            } => {
                let s = lp(vr, *start);
                let e = lp(vr, *end);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let _ = MoveToEx(hdc, s.x, s.y, None);
                let _ = LineTo(hdc, e.x, e.y);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Arrow {
                start,
                end,
                stroke,
                stroke_width,
            } => {
                let s = lp(vr, *start);
                let e = lp(vr, *end);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let _ = MoveToEx(hdc, s.x, s.y, None);
                let _ = LineTo(hdc, e.x, e.y);
                // Arrowhead — same geometry as the rasterizer (arrow_head_wings),
                // so the live preview matches the pinned/saved image.
                if let Some((left, right)) = arrow_head_wings(*start, *end, *stroke_width) {
                    let l = lp(vr, left);
                    let r = lp(vr, right);
                    let _ = MoveToEx(hdc, e.x, e.y, None);
                    let _ = LineTo(hdc, l.x, l.y);
                    let _ = MoveToEx(hdc, e.x, e.y, None);
                    let _ = LineTo(hdc, r.x, r.y);
                }
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Brush {
                points,
                stroke,
                stroke_width,
            } => {
                if points.len() < 2 {
                    return;
                }
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let first = lp(vr, points[0]);
                let _ = MoveToEx(hdc, first.x, first.y, None);
                for p in &points[1..] {
                    let p = lp(vr, *p);
                    let _ = LineTo(hdc, p.x, p.y);
                }
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Text {
                origin,
                text,
                color,
                ..
            } => draw_text_overlay(hdc, vr, *origin, text, *color),
            AnnotationShape::Mosaic { rect, .. } => {
                let rect = lr(vr, *rect);
                let pen = CreatePen(PS_SOLID, 2, COLORREF(0x0000D7FF));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                let _ = Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Number {
                center,
                value,
                fill,
                text,
            } => {
                let p = lp(vr, *center);
                let rect = RECT {
                    left: p.x - 12,
                    top: p.y - 12,
                    right: p.x + 12,
                    bottom: p.y + 12,
                };
                let brush = CreateSolidBrush(colorref(*fill));
                let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
                let _ = Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom);
                SelectObject(hdc, old_brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let mut label = wide(&value.to_string());
                let mut text_rect = rect;
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, colorref(*text));
                let _ = DrawTextW(
                    hdc,
                    &mut label,
                    &mut text_rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
        }
    }

    unsafe fn draw_text_overlay(hdc: HDC, vr: Rect, origin: Point, text: &str, color: Color) {
        let p = lp(vr, origin);
        let mut rect = RECT {
            left: p.x,
            top: p.y,
            right: vr.width as i32,
            bottom: vr.height as i32,
        };
        let mut wtext = wide(if text.is_empty() { "|" } else { text });
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, colorref(color));
        let _ = DrawTextW(hdc, &mut wtext, &mut rect, DT_SINGLELINE);
    }

    unsafe fn paint(hwnd: HWND, state: &OverlayState) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let w = state.virtual_rect.width as i32;
        let h = state.virtual_rect.height as i32;

        let mem_dc = CreateCompatibleDC(hdc);
        if mem_dc.0.is_null() {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        let bitmap = CreateCompatibleBitmap(hdc, w, h);
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        // 1. Frozen screenshot, 2. dim everything.
        let _ = BitBlt(mem_dc, 0, 0, w, h, state.shot_dc, 0, 0, SRCCOPY);
        dim(mem_dc, w, h);

        if let Some(sel) = active_rect(state) {
            let lx = sel.x - state.virtual_rect.x;
            let ly = sel.y - state.virtual_rect.y;
            let sw = sel.width as i32;
            let sh = sel.height as i32;

            // 3. Restore the selection to full brightness.
            let _ = BitBlt(mem_dc, lx, ly, sw, sh, state.shot_dc, lx, ly, SRCCOPY);

            let local = RECT {
                left: lx,
                top: ly,
                right: lx + sw,
                bottom: ly + sh,
            };

            // 3b. Annotations, clipped to the selection.
            let vr = state.virtual_rect;
            let _ = IntersectClipRect(mem_dc, local.left, local.top, local.right, local.bottom);
            for shape in state.doc.shapes() {
                draw_shape_overlay(mem_dc, vr, shape);
            }
            if let Some(shape) = overlay_preview_shape(state) {
                draw_shape_overlay(mem_dc, vr, &shape);
            }
            if let Some(origin) = state.text_origin {
                draw_text_overlay(mem_dc, vr, origin, &state.text_buffer, state.color);
            }
            SelectClipRgn(mem_dc, HRGN::default());

            let is_new = matches!(state.drag, Drag::NewSelection);
            let pen_color = if is_new {
                COLORREF(0x0000D7FF)
            } else {
                COLORREF(0x0000FF66)
            };
            draw_border(mem_dc, &local, pen_color);

            let editing_idle = state.interactive
                && state.phase == Phase::Editing
                && state.selection.is_some()
                && !is_new;
            // Resize handles only make sense in selection mode (no active tool).
            if editing_idle && state.active_tool.is_none() {
                draw_handles(mem_dc, &local);
            }

            draw_size_label(mem_dc, &local, sel.width, sel.height);

            // 4. Toolbar hole — only while idle (not dragging / not drawing).
            if editing_idle && matches!(state.drag, Drag::None) && state.draw_start.is_none() {
                let tr = toolbar_rect(state, sel);
                let hole = RECT {
                    left: tr.x - state.virtual_rect.x,
                    top: tr.y - state.virtual_rect.y,
                    right: tr.x - state.virtual_rect.x + tr.width as i32,
                    bottom: tr.y - state.virtual_rect.y + tr.height as i32,
                };
                let brush = CreateSolidBrush(COLOR_KEY);
                let _ = FillRect(mem_dc, &hole, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
            }
        }

        let _ = BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }

    unsafe fn dim(dc: HDC, w: i32, h: i32) {
        let dim_dc = CreateCompatibleDC(dc);
        if dim_dc.0.is_null() {
            return;
        }
        let bmp = CreateCompatibleBitmap(dc, 1, 1);
        let old = SelectObject(dim_dc, HGDIOBJ(bmp.0));
        let black = HBRUSH(GetStockObject(BLACK_BRUSH).0);
        let cell = RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        let _ = FillRect(dim_dc, &cell, black);
        let blend = BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: DIM_ALPHA,
            AlphaFormat: 0,
        };
        let _ = AlphaBlend(dc, 0, 0, w, h, dim_dc, 0, 0, 1, 1, blend);
        SelectObject(dim_dc, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(dim_dc);
    }

    unsafe fn draw_border(dc: HDC, rect: &RECT, color: COLORREF) {
        let pen = CreatePen(PS_SOLID, 2, color);
        let old_pen = SelectObject(dc, HGDIOBJ(pen.0));
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        SetBkMode(dc, TRANSPARENT);
        let _ = Rectangle(dc, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }

    unsafe fn draw_handles(dc: HDC, rect: &RECT) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let points = [
            (rect.left, rect.top),
            (cx, rect.top),
            (rect.right, rect.top),
            (rect.right, cy),
            (rect.right, rect.bottom),
            (cx, rect.bottom),
            (rect.left, rect.bottom),
            (rect.left, cy),
        ];
        let border = CreateSolidBrush(COLORREF(0x00303030));
        let fill = CreateSolidBrush(COLORREF(0x00FFFFFF));
        for (x, y) in points {
            let outer = RECT {
                left: x - HANDLE_HALF,
                top: y - HANDLE_HALF,
                right: x + HANDLE_HALF,
                bottom: y + HANDLE_HALF,
            };
            let _ = FillRect(dc, &outer, border);
            let inner = RECT {
                left: outer.left + 1,
                top: outer.top + 1,
                right: outer.right - 1,
                bottom: outer.bottom - 1,
            };
            let _ = FillRect(dc, &inner, fill);
        }
        let _ = DeleteObject(HGDIOBJ(border.0));
        let _ = DeleteObject(HGDIOBJ(fill.0));
    }

    unsafe fn draw_size_label(dc: HDC, rect: &RECT, width: u32, height: u32) {
        let label = format!("{width} x {height}");
        let mut text = wide(&label);
        // wide() appends a NUL; DrawTextW with -1 length would include it, so
        // pass the explicit character count instead.
        let char_count = label.chars().count() as i32;
        let label_width = (label.len() as i32 * 8).max(76);
        let mut label_rect = RECT {
            left: rect.left,
            top: rect.top - 27,
            right: rect.left + label_width,
            bottom: rect.top - 5,
        };
        if label_rect.top < 4 {
            label_rect.top = rect.bottom + 5;
            label_rect.bottom = rect.bottom + 27;
        }

        let bg = CreateSolidBrush(COLORREF(0x00202020));
        let _ = FillRect(dc, &label_rect, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        SetBkMode(dc, TRANSPARENT);
        SetBkColor(dc, COLORREF(0x00202020));
        SetTextColor(dc, COLORREF(0x00FFFFFF));
        let _ = DrawTextW(
            dc,
            &mut text[..char_count as usize],
            &mut label_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }

    fn selection_rect_from(start: POINT, current: POINT) -> Option<Rect> {
        let left = start.x.min(current.x);
        let top = start.y.min(current.y);
        let right = start.x.max(current.x);
        let bottom = start.y.max(current.y);
        let width = u32::try_from(right - left).ok()?;
        let height = u32::try_from(bottom - top).ok()?;
        if width < 2 || height < 2 {
            return None;
        }
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }

    // Spawn the (blocking) decide closure on its own thread so it never starves
    // the overlay message loop. Used by the non-interactive path.
    fn spawn_decide(
        hwnd: HWND,
        decide_fn: Box<dyn FnOnce(Rect) -> SelectionDecision + Send>,
        rect: Rect,
    ) {
        let hwnd_val = hwnd.0 as usize;
        std::thread::spawn(move || {
            let decision = decide_fn(rect);
            post_decision(hwnd_val, decision);
        });
    }

    unsafe fn cursor_point() -> POINT {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        point
    }

    unsafe fn window_rect_from_point(overlay_hwnd: HWND, point: POINT) -> Option<Rect> {
        struct HitTest {
            overlay_hwnd: HWND,
            point: POINT,
            result: Option<Rect>,
        }

        unsafe extern "system" fn enum_proc(
            hwnd: HWND,
            data: LPARAM,
        ) -> windows::Win32::Foundation::BOOL {
            let hit = &mut *(data.0 as *mut HitTest);
            if hwnd == hit.overlay_hwnd
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
            {
                return windows::Win32::Foundation::BOOL(1);
            }

            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return windows::Win32::Foundation::BOOL(1);
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width < 32 || height < 32 {
                return windows::Win32::Foundation::BOOL(1);
            }

            if hit.point.x >= rect.left
                && hit.point.x <= rect.right
                && hit.point.y >= rect.top
                && hit.point.y <= rect.bottom
            {
                hit.result = Some(Rect {
                    x: rect.left,
                    y: rect.top,
                    width: width as u32,
                    height: height as u32,
                });
                return windows::Win32::Foundation::BOOL(0);
            }

            windows::Win32::Foundation::BOOL(1)
        }

        let mut hit = HitTest {
            overlay_hwnd,
            point,
            result: None,
        };
        let _ = EnumWindows(Some(enum_proc), LPARAM((&mut hit as *mut HitTest) as isize));
        hit.result
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

// ─── RegionIndicator ──────────────────────────────────────────────────────────

/// A transparent, click-through border drawn around a screen region to show an
/// active recording area. Hidden from Windows Graphics Capture via
/// `WDA_EXCLUDEFROMCAPTURE`, so it does not appear in the recorded video.
pub struct RegionIndicator {
    #[cfg(windows)]
    #[allow(dead_code)]
    inner: windows_indicator::IndicatorHandle,
    #[cfg(not(windows))]
    _p: std::marker::PhantomData<()>,
}

impl RegionIndicator {
    pub fn new(rect: crate::Rect) -> crate::Result<Self> {
        #[cfg(windows)]
        {
            Ok(RegionIndicator {
                inner: windows_indicator::IndicatorHandle::new(rect)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = rect;
            Err(crate::WinScreenError::UnsupportedPlatform)
        }
    }
}

#[cfg(windows)]
mod windows_indicator {
    use crate::{Rect, WinScreenError};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HGDIOBJ, PAINTSTRUCT,
        InvalidateRect,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
        RegisterClassW, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, CS_HREDRAW,
        CS_VREDRAW, GWLP_USERDATA, IDC_ARROW, LWA_COLORKEY, MSG, SW_SHOWNA, WM_CLOSE,
        WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };
    use windows::core::PCWSTR;

    const CLASS_NAME: &str = "WinScreenRegionIndicator";
    // This exact color is used as the transparency key — pixels of this color
    // become invisible. Any other color renders as opaque.
    const COLOR_KEY: COLORREF = COLORREF(0x00030201);
    const BORDER: i32 = 3;
    const TIMER_MS: u32 = 700;
    const TIMER_ID: usize = 1;

    struct IndicatorState {
        width: i32,
        height: i32,
        phase: bool,
    }

    pub struct IndicatorHandle {
        // Store the raw HWND pointer value as usize for Send compatibility.
        // SAFETY: We only pass this to PostMessageW (safe from any thread) in Drop.
        hwnd: usize,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    unsafe impl Send for IndicatorHandle {}
    unsafe impl Sync for IndicatorHandle {}

    impl IndicatorHandle {
        pub fn new(rect: Rect) -> crate::Result<Self> {
            let (tx, rx) = crossbeam_channel::bounded::<crate::Result<usize>>(1);
            let thread = std::thread::Builder::new()
                .name("win-screen-region-indicator".to_string())
                .spawn(move || match create_window(rect) {
                    Ok(hwnd) => {
                        tx.send(Ok(hwnd)).ok();
                        run_loop();
                    }
                    Err(e) => {
                        tx.send(Err(e)).ok();
                    }
                })
                .map_err(|e| WinScreenError::Recording(e.to_string()))?;

            let hwnd = rx
                .recv()
                .map_err(|_| WinScreenError::Recording("indicator thread died".into()))??;

            Ok(IndicatorHandle {
                hwnd,
                thread: Some(thread),
            })
        }
    }

    impl Drop for IndicatorHandle {
        fn drop(&mut self) {
            unsafe {
                let hwnd = HWND(self.hwnd as *mut std::ffi::c_void);
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn create_window(rect: Rect) -> crate::Result<usize> {
        let class_name = wide(CLASS_NAME);
        let hinstance = unsafe { GetModuleHandleW(None) }?;

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }?,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        unsafe { RegisterClassW(&wnd_class) };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(wide("").as_ptr()),
                WS_POPUP,
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                None,
                None,
                hinstance,
                None,
            )
        }?;

        let state = Box::new(IndicatorState {
            width: rect.width as i32,
            height: rect.height as i32,
            phase: false,
        });

        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            SetLayeredWindowAttributes(hwnd, COLOR_KEY, 0, LWA_COLORKEY)?;
            // Best-effort: exclude from WGC capture (requires Win10 2004+)
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
            SetTimer(hwnd, TIMER_ID, TIMER_MS, None);
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }

        Ok(hwnd.0 as usize)
    }

    fn run_loop() {
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut IndicatorState;
        match msg {
            WM_TIMER => {
                if let Some(s) = state_ptr.as_mut() {
                    s.phase = !s.phase;
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }
            WM_PAINT => {
                if let Some(s) = state_ptr.as_ref() {
                    paint(hwnd, s);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_CLOSE => {
                let _ = KillTimer(hwnd, TIMER_ID);
                // Zero GWLP_USERDATA before dropping to prevent re-entrant use.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !state_ptr.is_null() {
                    drop(Box::from_raw(state_ptr));
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    unsafe fn paint(hwnd: HWND, s: &IndicatorState) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let w = s.width;
        let h = s.height;

        // Interior: transparent key color
        let key_brush = CreateSolidBrush(COLOR_KEY);
        let _ = FillRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: h }, key_brush);
        let _ = DeleteObject(HGDIOBJ(key_brush.0));

        // Border: pulse between two shades of red-orange
        // COLORREF bytes are 0x00BBGGRR (little-endian: R is lowest byte)
        let border_color = if s.phase {
            COLORREF(0x000000FF) // pure red
        } else {
            COLORREF(0x000055FF) // red-orange (#FF5500)
        };
        let b = CreateSolidBrush(border_color);
        let _ = FillRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: BORDER }, b);
        let _ = FillRect(hdc, &RECT { left: 0, top: h - BORDER, right: w, bottom: h }, b);
        let _ = FillRect(hdc, &RECT { left: 0, top: BORDER, right: BORDER, bottom: h - BORDER }, b);
        let _ = FillRect(hdc, &RECT { left: w - BORDER, top: BORDER, right: w, bottom: h - BORDER }, b);
        let _ = DeleteObject(HGDIOBJ(b.0));

        let _ = EndPaint(hwnd, &ps);
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
