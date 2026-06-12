use crate::{CapturedImage, Rect, Result, WinScreenError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationTool {
    Rectangle,
    Ellipse,
    Arrow,
    Line,
    Brush,
    Text,
    Mosaic,
    Number,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationEditAction {
    Confirm,
    Pin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationEditResult {
    pub image: CapturedImage,
    pub action: AnnotationEditAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const RED: Self = Self {
        r: 255,
        g: 64,
        b: 64,
        a: 255,
    };
    pub const YELLOW: Self = Self {
        r: 255,
        g: 214,
        b: 64,
        a: 255,
    };
    pub const BLACK: Self = Self {
        r: 20,
        g: 20,
        b: 20,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AnnotationShape {
    Rectangle {
        rect: Rect,
        stroke: Color,
        stroke_width: u32,
    },
    Ellipse {
        rect: Rect,
        stroke: Color,
        stroke_width: u32,
    },
    Arrow {
        start: Point,
        end: Point,
        stroke: Color,
        stroke_width: u32,
    },
    Line {
        start: Point,
        end: Point,
        stroke: Color,
        stroke_width: u32,
    },
    Brush {
        points: Vec<Point>,
        stroke: Color,
        stroke_width: u32,
    },
    Text {
        origin: Point,
        text: String,
        color: Color,
        font_size: u32,
    },
    Mosaic {
        rect: Rect,
        block_size: u32,
    },
    Number {
        center: Point,
        value: u32,
        fill: Color,
        text: Color,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnnotationDocument {
    shapes: Vec<AnnotationShape>,
    undone: Vec<AnnotationShape>,
}

impl AnnotationDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shapes(&self) -> &[AnnotationShape] {
        &self.shapes
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn push(&mut self, shape: AnnotationShape) {
        self.shapes.push(shape);
        self.undone.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(shape) = self.shapes.pop() else {
            return false;
        };
        self.undone.push(shape);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(shape) = self.undone.pop() else {
            return false;
        };
        self.shapes.push(shape);
        true
    }

    pub fn render(&self, image: &CapturedImage) -> Result<CapturedImage> {
        let mut output = image.clone();
        for shape in &self.shapes {
            shape.apply(&mut output)?;
        }
        Ok(output)
    }
}

pub trait Annotation {
    fn apply(&self, image: &mut CapturedImage) -> Result<()>;
}

impl Annotation for AnnotationShape {
    fn apply(&self, image: &mut CapturedImage) -> Result<()> {
        match self {
            AnnotationShape::Rectangle {
                rect,
                stroke,
                stroke_width,
            } => draw_rect(image, *rect, *stroke, *stroke_width),
            AnnotationShape::Ellipse {
                rect,
                stroke,
                stroke_width,
            } => draw_ellipse(image, *rect, *stroke, *stroke_width),
            AnnotationShape::Arrow {
                start,
                end,
                stroke,
                stroke_width,
            } => {
                draw_line(image, *start, *end, *stroke, *stroke_width);
                draw_arrow_head(image, *start, *end, *stroke, *stroke_width);
                Ok(())
            }
            AnnotationShape::Line {
                start,
                end,
                stroke,
                stroke_width,
            } => {
                draw_line(image, *start, *end, *stroke, *stroke_width);
                Ok(())
            }
            AnnotationShape::Brush {
                points,
                stroke,
                stroke_width,
            } => {
                for pair in points.windows(2) {
                    draw_line(image, pair[0], pair[1], *stroke, *stroke_width);
                }
                Ok(())
            }
            AnnotationShape::Text {
                origin,
                text,
                color,
                font_size,
            } => render_text(image, *origin, text, *color, *font_size),
            AnnotationShape::Mosaic { rect, block_size } => mosaic(image, *rect, *block_size),
            AnnotationShape::Number {
                center,
                value,
                fill,
                text,
            } => {
                draw_number(image, *center, *value, *fill, *text);
                Ok(())
            }
        }
    }
}

pub fn edit_image(image: CapturedImage) -> Result<Option<CapturedImage>> {
    Ok(edit_image_with_action(image)?.map(|result| result.image))
}

pub fn edit_image_with_action(image: CapturedImage) -> Result<Option<AnnotationEditResult>> {
    edit_image_with_action_at(image, None)
}

pub fn edit_image_with_action_at(
    image: CapturedImage,
    anchor: Option<Rect>,
) -> Result<Option<AnnotationEditResult>> {
    #[cfg(windows)]
    {
        return windows_editor::edit_image_with_action(image, anchor);
    }

    #[cfg(not(windows))]
    {
        let _ = image;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

fn normalize_rect(start: Point, end: Point) -> Option<Rect> {
    let left = start.x.min(end.x);
    let top = start.y.min(end.y);
    let right = start.x.max(end.x);
    let bottom = start.y.max(end.y);
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

fn draw_rect(image: &mut CapturedImage, rect: Rect, color: Color, width: u32) -> Result<()> {
    validate_rect_for_image(image, rect)?;
    let width = width.max(1) as i32;
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width as i32 - 1;
    let bottom = rect.y + rect.height as i32 - 1;
    for offset in 0..width {
        draw_line(
            image,
            Point {
                x: left + offset,
                y: top + offset,
            },
            Point {
                x: right - offset,
                y: top + offset,
            },
            color,
            1,
        );
        draw_line(
            image,
            Point {
                x: left + offset,
                y: bottom - offset,
            },
            Point {
                x: right - offset,
                y: bottom - offset,
            },
            color,
            1,
        );
        draw_line(
            image,
            Point {
                x: left + offset,
                y: top + offset,
            },
            Point {
                x: left + offset,
                y: bottom - offset,
            },
            color,
            1,
        );
        draw_line(
            image,
            Point {
                x: right - offset,
                y: top + offset,
            },
            Point {
                x: right - offset,
                y: bottom - offset,
            },
            color,
            1,
        );
    }
    Ok(())
}

fn draw_ellipse(image: &mut CapturedImage, rect: Rect, color: Color, width: u32) -> Result<()> {
    validate_rect_for_image(image, rect)?;
    let cx = rect.x as f32 + rect.width as f32 / 2.0;
    let cy = rect.y as f32 + rect.height as f32 / 2.0;
    let rx = (rect.width as f32 / 2.0).max(1.0);
    let ry = (rect.height as f32 / 2.0).max(1.0);
    let thickness = (width.max(1) as f32 / rx.min(ry)).max(0.001);

    for y in rect.y..(rect.y + rect.height as i32) {
        for x in rect.x..(rect.x + rect.width as i32) {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            let d = dx * dx + dy * dy;
            if (1.0 - thickness..=1.0 + thickness).contains(&d) {
                set_pixel(image, x, y, color);
            }
        }
    }
    Ok(())
}

fn draw_line(image: &mut CapturedImage, start: Point, end: Point, color: Color, width: u32) {
    let mut x0 = start.x;
    let mut y0 = start.y;
    let x1 = end.x;
    let y1 = end.y;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_dot(image, x0, y0, color, width);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_arrow_head(image: &mut CapturedImage, start: Point, end: Point, color: Color, width: u32) {
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 2.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;
    let head_len = 14.0_f32.max(width as f32 * 4.0);
    let wing = 0.55_f32;
    let left = Point {
        x: (end.x as f32 - head_len * (ux + uy * wing)).round() as i32,
        y: (end.y as f32 - head_len * (uy - ux * wing)).round() as i32,
    };
    let right = Point {
        x: (end.x as f32 - head_len * (ux - uy * wing)).round() as i32,
        y: (end.y as f32 - head_len * (uy + ux * wing)).round() as i32,
    };
    draw_line(image, end, left, color, width);
    draw_line(image, end, right, color, width);
}

fn draw_dot(image: &mut CapturedImage, x: i32, y: i32, color: Color, width: u32) {
    let radius = (width.max(1) as i32 - 1) / 2;
    for yy in (y - radius)..=(y + radius) {
        for xx in (x - radius)..=(x + radius) {
            set_pixel(image, xx, yy, color);
        }
    }
}

fn draw_text_placeholder(
    image: &mut CapturedImage,
    origin: Point,
    text: &str,
    color: Color,
    font_size: u32,
) {
    let char_w = (font_size / 2).max(6) as i32;
    let char_h = font_size.max(12) as i32;
    for (idx, ch) in text.chars().enumerate() {
        if ch.is_whitespace() {
            continue;
        }
        let x = origin.x + idx as i32 * (char_w + 2);
        let rect = Rect {
            x,
            y: origin.y,
            width: char_w.max(2) as u32,
            height: char_h.max(2) as u32,
        };
        let _ = draw_rect(image, clip_rect_to_image(image, rect), color, 1);
    }
}

fn render_text(
    image: &mut CapturedImage,
    origin: Point,
    text: &str,
    color: Color,
    font_size: u32,
) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_text::render_text(image, origin, text, color, font_size);
    }

    #[cfg(not(windows))]
    {
        draw_text_placeholder(image, origin, text, color, font_size);
        Ok(())
    }
}

fn draw_number(image: &mut CapturedImage, center: Point, value: u32, fill: Color, text: Color) {
    let radius = 12_i32;
    for y in (center.y - radius)..=(center.y + radius) {
        for x in (center.x - radius)..=(center.x + radius) {
            let dx = x - center.x;
            let dy = y - center.y;
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(image, x, y, fill);
            }
        }
    }

    let label = value.to_string();
    let x = center.x - (label.len() as i32 * 3);
    let y = center.y - 5;
    draw_text_placeholder(image, Point { x, y }, &label, text, 10);
}

fn mosaic(image: &mut CapturedImage, rect: Rect, block_size: u32) -> Result<()> {
    validate_rect_for_image(image, rect)?;
    let block = block_size.max(4) as i32;
    let right = rect.x + rect.width as i32;
    let bottom = rect.y + rect.height as i32;

    let mut y = rect.y;
    while y < bottom {
        let mut x = rect.x;
        while x < right {
            let block_right = (x + block).min(right);
            let block_bottom = (y + block).min(bottom);
            let color = average_color(image, x, y, block_right, block_bottom);
            for yy in y..block_bottom {
                for xx in x..block_right {
                    set_pixel(image, xx, yy, color);
                }
            }
            x += block;
        }
        y += block;
    }
    Ok(())
}

fn average_color(image: &CapturedImage, left: i32, top: i32, right: i32, bottom: i32) -> Color {
    let mut r = 0_u64;
    let mut g = 0_u64;
    let mut b = 0_u64;
    let mut a = 0_u64;
    let mut count = 0_u64;

    for y in top..bottom {
        for x in left..right {
            if let Some(offset) = pixel_offset(image, x, y) {
                r += image.rgba[offset] as u64;
                g += image.rgba[offset + 1] as u64;
                b += image.rgba[offset + 2] as u64;
                a += image.rgba[offset + 3] as u64;
                count += 1;
            }
        }
    }

    if count == 0 {
        return Color::BLACK;
    }

    Color {
        r: (r / count) as u8,
        g: (g / count) as u8,
        b: (b / count) as u8,
        a: (a / count) as u8,
    }
}

fn validate_rect_for_image(image: &CapturedImage, rect: Rect) -> Result<()> {
    if rect.width == 0
        || rect.height == 0
        || rect.x < 0
        || rect.y < 0
        || rect.x as u32 >= image.width
        || rect.y as u32 >= image.height
        || rect.x as u32 + rect.width > image.width
        || rect.y as u32 + rect.height > image.height
    {
        return Err(WinScreenError::InvalidRect(rect));
    }
    Ok(())
}

fn clip_rect_to_image(image: &CapturedImage, rect: Rect) -> Rect {
    let left = rect.x.clamp(0, image.width.saturating_sub(1) as i32);
    let top = rect.y.clamp(0, image.height.saturating_sub(1) as i32);
    let right = (rect.x + rect.width as i32).clamp(left + 1, image.width as i32);
    let bottom = (rect.y + rect.height as i32).clamp(top + 1, image.height as i32);
    Rect {
        x: left,
        y: top,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    }
}

fn set_pixel(image: &mut CapturedImage, x: i32, y: i32, color: Color) {
    let Some(offset) = pixel_offset(image, x, y) else {
        return;
    };
    let alpha = color.a as u16;
    let inv = 255_u16 - alpha;
    image.rgba[offset] = ((color.r as u16 * alpha + image.rgba[offset] as u16 * inv) / 255) as u8;
    image.rgba[offset + 1] =
        ((color.g as u16 * alpha + image.rgba[offset + 1] as u16 * inv) / 255) as u8;
    image.rgba[offset + 2] =
        ((color.b as u16 * alpha + image.rgba[offset + 2] as u16 * inv) / 255) as u8;
    image.rgba[offset + 3] = 255;
}

fn pixel_offset(image: &CapturedImage, x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 || x as u32 >= image.width || y as u32 >= image.height {
        return None;
    }
    Some((y as usize * image.width as usize + x as usize) * 4)
}

#[cfg(windows)]
mod windows_text {
    use super::{Color, Point};
    use crate::{CapturedImage, Result, WinScreenError};
    use std::mem::size_of;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
        SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_NOPREFIX,
        DT_SINGLELINE, FF_DONTCARE, FW_NORMAL, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, PROOF_QUALITY,
        TRANSPARENT,
    };

    pub fn render_text(
        image: &mut CapturedImage,
        origin: Point,
        text: &str,
        color: Color,
        font_size: u32,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let bitmap_dc = unsafe { CreateCompatibleDC(HDC(null_mut())) };
        if bitmap_dc.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "CreateCompatibleDC for annotation text",
            });
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: image.width as i32,
                biHeight: -(image.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = null_mut();
        let bitmap =
            unsafe { CreateDIBSection(bitmap_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
        if bitmap.0.is_null() || bits.is_null() {
            unsafe {
                let _ = DeleteDC(bitmap_dc);
            }
            return Err(WinScreenError::NotImplemented {
                feature: "CreateDIBSection for annotation text",
            });
        }

        let mut bgra = image.rgba.clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
            let old_bitmap = SelectObject(bitmap_dc, HGDIOBJ(bitmap.0));
            let font_name = wide("Microsoft YaHei UI");
            let font = CreateFontW(
                -(font_size.max(10) as i32),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0.into(),
                OUT_DEFAULT_PRECIS.0.into(),
                CLIP_DEFAULT_PRECIS.0.into(),
                PROOF_QUALITY.0.into(),
                (DEFAULT_PITCH.0 | FF_DONTCARE.0).into(),
                PCWSTR(font_name.as_ptr()),
            );
            if font.0.is_null() {
                SelectObject(bitmap_dc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(bitmap_dc);
                return Err(WinScreenError::NotImplemented {
                    feature: "CreateFontW for annotation text",
                });
            }
            let old_font = SelectObject(bitmap_dc, HGDIOBJ(font.0));
            SetBkMode(bitmap_dc, TRANSPARENT);
            SetTextColor(
                bitmap_dc,
                COLORREF(color.r as u32 | ((color.g as u32) << 8) | ((color.b as u32) << 16)),
            );
            let mut rect = RECT {
                left: origin.x,
                top: origin.y,
                right: image.width as i32,
                bottom: image.height as i32,
            };
            let mut wide_text = wide(text);
            let _ = DrawTextW(
                bitmap_dc,
                &mut wide_text,
                &mut rect,
                DT_SINGLELINE | DT_NOPREFIX,
            );

            std::ptr::copy_nonoverlapping(bits as *const u8, bgra.as_mut_ptr(), bgra.len());
            SelectObject(bitmap_dc, old_font);
            SelectObject(bitmap_dc, old_bitmap);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(bitmap_dc);
        }

        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        image.rgba = bgra;
        Ok(())
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
mod windows_editor {
    use super::{
        normalize_rect, AnnotationDocument, AnnotationEditAction, AnnotationEditResult,
        AnnotationShape, AnnotationTool, Color, Point,
    };
    use crate::{platform, CapturedImage, Rect, Result, WinScreenError};
    use std::mem::size_of;
    use std::ptr::null_mut;
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection,
        CreatePen, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect, GetStockObject,
        InvalidateRect, MoveToEx, Rectangle, SelectObject, SetBkMode, SetTextColor, StretchBlt,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
        HBITMAP, HDC, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, GetWindowLongPtrW, LoadCursorW, PostQuitMessage, RegisterClassW,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
        IDC_CROSS, MSG, SW_SHOW, WM_CHAR, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "WinScreenAnnotationEditor";
    const TOOLBAR_HEIGHT: i32 = 42;

    struct EditorState {
        image: CapturedImage,
        bitmap: HBITMAP,
        bitmap_dc: HDC,
        doc: AnnotationDocument,
        tool: AnnotationTool,
        drawing_start: Option<Point>,
        current: Point,
        brush_points: Vec<Point>,
        text_origin: Option<Point>,
        text_buffer: String,
        number: u32,
        result: Option<Option<AnnotationEditResult>>,
        scale: f32,
        window_width: i32,
        window_height: i32,
        toolbar_ready_at: Instant,
    }

    pub fn edit_image_with_action(
        image: CapturedImage,
        anchor: Option<Rect>,
    ) -> Result<Option<AnnotationEditResult>> {
        platform::set_process_dpi_aware().ok();
        let (bitmap, bitmap_dc) = create_bitmap(&image)?;
        let virtual_rect = platform::virtual_screen_rect()?;
        let max_w = (virtual_rect.width as i32 - 160).max(320);
        let max_h = (virtual_rect.height as i32 - 140 - TOOLBAR_HEIGHT).max(240);
        let scale = (max_w as f32 / image.width as f32)
            .min(max_h as f32 / image.height as f32)
            .min(1.0);
        let image_w = (image.width as f32 * scale).round().max(1.0) as i32;
        let image_h = (image.height as f32 * scale).round().max(1.0) as i32;
        let window_width = image_w;
        let window_height = image_h + TOOLBAR_HEIGHT;

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

        let mut state = Box::new(EditorState {
            image,
            bitmap,
            bitmap_dc,
            doc: AnnotationDocument::new(),
            tool: AnnotationTool::Rectangle,
            drawing_start: None,
            current: Point { x: 0, y: 0 },
            brush_points: Vec::new(),
            text_origin: None,
            text_buffer: String::new(),
            number: 1,
            result: None,
            scale,
            window_width,
            window_height,
            toolbar_ready_at: Instant::now() + Duration::from_millis(250),
        });

        let (window_x, window_y) =
            editor_position(anchor, virtual_rect, window_width, window_height);

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(wide("win-screen annotate").as_ptr()),
                WS_POPUP | WS_VISIBLE,
                window_x,
                window_y,
                window_width,
                window_height,
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
                (&mut *state as *mut EditorState) as isize,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = InvalidateRect(hwnd, None, true);
        }

        let state_ptr = Box::into_raw(state);
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        let mut state = unsafe { Box::from_raw(state_ptr) };
        Ok(state.result.take().unwrap_or(None))
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState;

        match msg {
            WM_LBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    let point = image_point_from_cursor(hwnd, state);
                    if cursor_in_toolbar(hwnd, state) {
                        handle_toolbar_click(hwnd, state);
                        return LRESULT(0);
                    }
                    if matches!(state.tool, AnnotationTool::Text) {
                        state.text_origin = Some(point);
                        state.text_buffer.clear();
                    } else if matches!(state.tool, AnnotationTool::Number) {
                        state.doc.push(AnnotationShape::Number {
                            center: point,
                            value: state.number,
                            fill: Color::RED,
                            text: Color::WHITE,
                        });
                        state.number += 1;
                    } else {
                        state.drawing_start = Some(point);
                        state.current = point;
                        state.brush_points.clear();
                        state.brush_points.push(point);
                        SetCapture(hwnd);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.drawing_start.is_some() {
                        let point = image_point_from_cursor(hwnd, state);
                        state.current = point;
                        if matches!(state.tool, AnnotationTool::Brush) {
                            state.brush_points.push(point);
                        }
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                if let Some(state) = state_ptr.as_mut() {
                    if let Some(shape) = finish_shape(hwnd, state) {
                        state.doc.push(shape);
                    }
                    state.drawing_start = None;
                    state.brush_points.clear();
                    let _ = ReleaseCapture();
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_CHAR => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.text_origin.is_some() {
                        match char::from_u32(wparam.0 as u32) {
                            Some('\r') => commit_text(hwnd, state),
                            Some('\u{1b}') => {
                                state.text_origin = None;
                                state.text_buffer.clear();
                            }
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
                    if key_down(wparam, b'1') {
                        state.tool = AnnotationTool::Rectangle;
                    } else if key_down(wparam, b'2') {
                        state.tool = AnnotationTool::Ellipse;
                    } else if key_down(wparam, b'3') {
                        state.tool = AnnotationTool::Arrow;
                    } else if key_down(wparam, b'4') {
                        state.tool = AnnotationTool::Line;
                    } else if key_down(wparam, b'5') {
                        state.tool = AnnotationTool::Brush;
                    } else if key_down(wparam, b'6') {
                        state.tool = AnnotationTool::Text;
                    } else if key_down(wparam, b'7') {
                        state.tool = AnnotationTool::Mosaic;
                    } else if key_down(wparam, b'8') {
                        state.tool = AnnotationTool::Number;
                    } else if key_down(wparam, b'Z') && ctrl_pressed() {
                        state.doc.undo();
                    } else if key_down(wparam, b'Y') && ctrl_pressed() {
                        state.doc.redo();
                    } else if wparam.0 == VK_ESCAPE.0 as usize {
                        state.result = Some(None);
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    } else if wparam.0 == VK_RETURN.0 as usize {
                        if state.text_origin.is_some() {
                            commit_text(hwnd, state);
                        } else {
                            state.result = finish_edit(state, AnnotationEditAction::Confirm).ok();
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_RBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    state.result = Some(None);
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
            }
            WM_PAINT => {
                if let Some(state) = state_ptr.as_ref() {
                    paint(hwnd, state);
                    return LRESULT(0);
                }
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_DESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn editor_position(
        anchor: Option<Rect>,
        virtual_rect: Rect,
        window_width: i32,
        window_height: i32,
    ) -> (i32, i32) {
        let Some(anchor) = anchor else {
            return (virtual_rect.x + 80, virtual_rect.y + 80);
        };

        let min_x = virtual_rect.x;
        let min_y = virtual_rect.y;
        let max_x = virtual_rect.x + virtual_rect.width as i32 - window_width;
        let max_y = virtual_rect.y + virtual_rect.height as i32 - window_height;
        let x = anchor.x.clamp(min_x, max_x.max(min_x));
        let above_y = anchor.y - TOOLBAR_HEIGHT;
        let below_y = anchor.y + anchor.height as i32;
        let y = if above_y >= min_y { above_y } else { below_y }.clamp(min_y, max_y.max(min_y));

        (x, y)
    }

    unsafe fn paint(hwnd: HWND, state: &EditorState) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mem_dc = CreateCompatibleDC(hdc);
        if mem_dc.0.is_null() {
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        let bitmap = CreateCompatibleBitmap(hdc, state.window_width, state.window_height);
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            return;
        }
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        let bg = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(0x00202020));
        let client = RECT {
            left: 0,
            top: 0,
            right: state.window_width,
            bottom: state.window_height,
        };
        let _ = FillRect(mem_dc, &client, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        let image_h = state.window_height - TOOLBAR_HEIGHT;
        let _ = StretchBlt(
            mem_dc,
            0,
            TOOLBAR_HEIGHT,
            state.window_width,
            image_h,
            state.bitmap_dc,
            0,
            0,
            state.image.width as i32,
            state.image.height as i32,
            SRCCOPY,
        );

        for shape in state.doc.shapes() {
            draw_shape_preview(mem_dc, state, shape);
        }
        if let Some(shape) = preview_shape(state) {
            draw_shape_preview(mem_dc, state, &shape);
        }
        if let Some(origin) = state.text_origin {
            draw_text_preview(mem_dc, state, origin, &state.text_buffer);
        }
        draw_toolbar(mem_dc, state);

        let _ = BitBlt(
            hdc,
            0,
            0,
            state.window_width,
            state.window_height,
            mem_dc,
            0,
            0,
            SRCCOPY,
        );
        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }

    unsafe fn draw_toolbar(hdc: HDC, state: &EditorState) {
        let tools = [
            ("1 Rect", AnnotationTool::Rectangle),
            ("2 Oval", AnnotationTool::Ellipse),
            ("3 Arrow", AnnotationTool::Arrow),
            ("4 Line", AnnotationTool::Line),
            ("5 Pen", AnnotationTool::Brush),
            ("6 Text", AnnotationTool::Text),
            ("7 Mosaic", AnnotationTool::Mosaic),
            ("8 Num", AnnotationTool::Number),
            ("Enter OK", state.tool),
            ("Pin", state.tool),
            ("Esc Cancel", state.tool),
        ];
        for (idx, (label, tool)) in tools.iter().enumerate() {
            let left = idx as i32 * 84;
            let mut rect = RECT {
                left,
                top: 0,
                right: left + 82,
                bottom: TOOLBAR_HEIGHT - 2,
            };
            let selected = idx < 8 && *tool == state.tool;
            let color = if selected { 0x00464646 } else { 0x002C2C2C };
            let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(color));
            let _ = FillRect(hdc, &rect, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));

            let mut text = wide(label);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FFFFFF));
            let _ = DrawTextW(
                hdc,
                &mut text,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }

    unsafe fn draw_shape_preview(hdc: HDC, state: &EditorState, shape: &AnnotationShape) {
        match shape {
            AnnotationShape::Rectangle {
                rect,
                stroke,
                stroke_width,
            } => {
                let rect = scaled_rect(state, *rect);
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
                let rect = scaled_rect(state, *rect);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                let _ = Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Arrow {
                start,
                end,
                stroke,
                stroke_width,
            }
            | AnnotationShape::Line {
                start,
                end,
                stroke,
                stroke_width,
            } => {
                let start = scaled_point(state, *start);
                let end = scaled_point(state, *end);
                let pen = CreatePen(PS_SOLID, *stroke_width as i32, colorref(*stroke));
                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                let _ = MoveToEx(hdc, start.x, start.y, None);
                let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, end.x, end.y);
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
                let first = scaled_point(state, points[0]);
                let _ = MoveToEx(hdc, first.x, first.y, None);
                for point in &points[1..] {
                    let point = scaled_point(state, *point);
                    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, point.x, point.y);
                }
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
            AnnotationShape::Text { origin, text, .. } => {
                draw_text_preview(hdc, state, *origin, text)
            }
            AnnotationShape::Mosaic { rect, .. } => {
                let rect = scaled_rect(state, *rect);
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
                let point = scaled_point(state, *center);
                let rect = RECT {
                    left: point.x - 12,
                    top: point.y - 12,
                    right: point.x + 12,
                    bottom: point.y + 12,
                };
                let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(colorref(*fill));
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

    unsafe fn draw_text_preview(hdc: HDC, state: &EditorState, origin: Point, text: &str) {
        let point = scaled_point(state, origin);
        let mut rect = RECT {
            left: point.x,
            top: point.y,
            right: state.window_width,
            bottom: state.window_height,
        };
        let mut text = wide(if text.is_empty() { "|" } else { text });
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x000040FF));
        let _ = DrawTextW(hdc, &mut text, &mut rect, DT_SINGLELINE);
    }

    fn finish_shape(hwnd: HWND, state: &mut EditorState) -> Option<AnnotationShape> {
        state.current = unsafe { image_point_from_cursor(hwnd, state) };
        preview_shape(state)
    }

    fn preview_shape(state: &EditorState) -> Option<AnnotationShape> {
        let start = state.drawing_start?;
        match state.tool {
            AnnotationTool::Rectangle => Some(AnnotationShape::Rectangle {
                rect: normalize_rect(start, state.current)?,
                stroke: Color::RED,
                stroke_width: 3,
            }),
            AnnotationTool::Ellipse => Some(AnnotationShape::Ellipse {
                rect: normalize_rect(start, state.current)?,
                stroke: Color::RED,
                stroke_width: 3,
            }),
            AnnotationTool::Arrow => Some(AnnotationShape::Arrow {
                start,
                end: state.current,
                stroke: Color::RED,
                stroke_width: 3,
            }),
            AnnotationTool::Line => Some(AnnotationShape::Line {
                start,
                end: state.current,
                stroke: Color::RED,
                stroke_width: 3,
            }),
            AnnotationTool::Brush => Some(AnnotationShape::Brush {
                points: state.brush_points.clone(),
                stroke: Color::YELLOW,
                stroke_width: 4,
            }),
            AnnotationTool::Mosaic => Some(AnnotationShape::Mosaic {
                rect: normalize_rect(start, state.current)?,
                block_size: 12,
            }),
            AnnotationTool::Text | AnnotationTool::Number => None,
        }
    }

    fn commit_text(hwnd: HWND, state: &mut EditorState) {
        let Some(origin) = state.text_origin.take() else {
            return;
        };
        if !state.text_buffer.trim().is_empty() {
            state.doc.push(AnnotationShape::Text {
                origin,
                text: state.text_buffer.clone(),
                color: Color::RED,
                font_size: 22,
            });
        }
        state.text_buffer.clear();
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }

    fn finish_edit(
        state: &EditorState,
        action: AnnotationEditAction,
    ) -> Result<Option<AnnotationEditResult>> {
        Ok(Some(AnnotationEditResult {
            image: state.doc.render(&state.image)?,
            action,
        }))
    }

    unsafe fn handle_toolbar_click(hwnd: HWND, state: &mut EditorState) {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let idx = (point.x - window_origin_x(hwnd)) / 84;
        match idx {
            0 => state.tool = AnnotationTool::Rectangle,
            1 => state.tool = AnnotationTool::Ellipse,
            2 => state.tool = AnnotationTool::Arrow,
            3 => state.tool = AnnotationTool::Line,
            4 => state.tool = AnnotationTool::Brush,
            5 => state.tool = AnnotationTool::Text,
            6 => state.tool = AnnotationTool::Mosaic,
            7 => state.tool = AnnotationTool::Number,
            8 => {
                state.result = finish_edit(state, AnnotationEditAction::Confirm).ok();
                let _ = DestroyWindow(hwnd);
            }
            9 => {
                state.result = finish_edit(state, AnnotationEditAction::Pin).ok();
                let _ = DestroyWindow(hwnd);
            }
            10 => {
                state.result = Some(None);
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
        let _ = InvalidateRect(hwnd, None, false);
    }

    unsafe fn cursor_in_toolbar(hwnd: HWND, state: &EditorState) -> bool {
        if Instant::now() < state.toolbar_ready_at {
            return false;
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let local_x = point.x - window_origin_x(hwnd);
        let local_y = point.y - window_origin_y(hwnd);
        local_x >= 0 && local_x < state.window_width && (0..TOOLBAR_HEIGHT).contains(&local_y)
    }

    unsafe fn image_point_from_cursor(hwnd: HWND, state: &EditorState) -> Point {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let x = ((point.x - window_origin_x(hwnd)) as f32 / state.scale).round() as i32;
        let y = ((point.y - window_origin_y(hwnd) - TOOLBAR_HEIGHT) as f32 / state.scale).round()
            as i32;
        Point {
            x: x.clamp(0, state.image.width.saturating_sub(1) as i32),
            y: y.clamp(0, state.image.height.saturating_sub(1) as i32),
        }
    }

    unsafe fn window_origin_x(hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect);
        rect.left
    }

    unsafe fn window_origin_y(hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect);
        rect.top
    }

    fn scaled_point(state: &EditorState, point: Point) -> Point {
        Point {
            x: (point.x as f32 * state.scale).round() as i32,
            y: TOOLBAR_HEIGHT + (point.y as f32 * state.scale).round() as i32,
        }
    }

    fn scaled_rect(state: &EditorState, rect: Rect) -> RECT {
        let left = (rect.x as f32 * state.scale).round() as i32;
        let top = TOOLBAR_HEIGHT + (rect.y as f32 * state.scale).round() as i32;
        RECT {
            left,
            top,
            right: left + (rect.width as f32 * state.scale).round() as i32,
            bottom: top + (rect.height as f32 * state.scale).round() as i32,
        }
    }

    fn key_down(wparam: WPARAM, key: u8) -> bool {
        wparam.0 == key as usize
    }

    fn ctrl_pressed() -> bool {
        unsafe {
            (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) as u16 & 0x8000) != 0
        }
    }

    fn colorref(color: Color) -> COLORREF {
        COLORREF(color.r as u32 | ((color.g as u32) << 8) | ((color.b as u32) << 16))
    }

    fn create_bitmap(image: &CapturedImage) -> Result<(HBITMAP, HDC)> {
        let bitmap_dc = unsafe { CreateCompatibleDC(HDC(null_mut())) };
        if bitmap_dc.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "CreateCompatibleDC for annotation bitmap",
            });
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: image.width as i32,
                biHeight: -(image.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = null_mut();
        let bitmap =
            unsafe { CreateDIBSection(bitmap_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
        if bitmap.0.is_null() || bits.is_null() {
            unsafe {
                let _ = DeleteDC(bitmap_dc);
            }
            return Err(WinScreenError::NotImplemented {
                feature: "CreateDIBSection for annotation bitmap",
            });
        }

        let mut bgra = image.rgba.clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
            SelectObject(bitmap_dc, HGDIOBJ(bitmap.0));
        }

        Ok((bitmap, bitmap_dc))
    }

    impl Drop for EditorState {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                let _ = DeleteDC(self.bitmap_dc);
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_round_trip() {
        let mut doc = AnnotationDocument::new();
        doc.push(AnnotationShape::Line {
            start: Point { x: 0, y: 0 },
            end: Point { x: 3, y: 3 },
            stroke: Color::RED,
            stroke_width: 1,
        });
        assert_eq!(doc.shapes().len(), 1);
        assert!(doc.undo());
        assert!(doc.is_empty());
        assert!(doc.redo());
        assert_eq!(doc.shapes().len(), 1);
    }

    #[test]
    fn mosaic_changes_target_region() {
        let mut rgba = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                rgba.extend_from_slice(&[(x * 40) as u8, (y * 40) as u8, 0, 255]);
            }
        }
        let image = CapturedImage::new(4, 4, rgba).unwrap();
        let mut doc = AnnotationDocument::new();
        doc.push(AnnotationShape::Mosaic {
            rect: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            block_size: 4,
        });
        let rendered = doc.render(&image).unwrap();
        assert_ne!(rendered.rgba, image.rgba);
        assert_eq!(&rendered.rgba[0..4], &rendered.rgba[4..8]);
    }
}
