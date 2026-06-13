use crate::{Rect, Result};

// ─── Global hotkey ─────────────────────────────────────────────────────────────

/// Hotkey modifier flags — compose with bitwise OR.
pub mod hotkey_mod {
    pub const ALT: u32 = 0x0001;
    pub const CONTROL: u32 = 0x0002;
    pub const SHIFT: u32 = 0x0004;
    pub const WIN: u32 = 0x0008;
    /// Suppress repeated WM_HOTKEY messages while the key is held.
    pub const NOREPEAT: u32 = 0x4000;
}

/// A registered global hotkey. Unregisters and stops the listener thread on drop.
pub struct HotkeyHandle {
    thread_id: u32,
    hotkey_id: i32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HotkeyHandle {
    /// The hotkey ID passed to [`register_hotkey`].
    pub fn id(&self) -> i32 {
        self.hotkey_id
    }

    /// Manually unregister the hotkey and stop the listener thread.
    pub fn unregister(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, None, None);
        }
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

impl Drop for HotkeyHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Register a global hotkey that calls `callback` each time it fires.
///
/// `id` must be unique per process (1–0xBFFF). `modifiers` is a combination
/// of [`hotkey_mod`] flags. `vk` is a Windows virtual-key code (e.g. `0x2C`
/// for Print Screen).
///
/// The callback runs on a dedicated background thread — keep it lightweight
/// (e.g. send on a channel) and do not block.
#[cfg(windows)]
pub fn register_hotkey<F>(id: i32, modifiers: u32, vk: u32, callback: F) -> Result<HotkeyHandle>
where
    F: Fn() + Send + 'static,
{
    use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS},
        WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY},
    };

    let thread_id = Arc::new(AtomicU32::new(0));
    let thread_id_clone = thread_id.clone();
    let hotkey_id = id;

    let thread = std::thread::spawn(move || unsafe {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        thread_id_clone.store(GetCurrentThreadId(), Ordering::Release);

        if RegisterHotKey(None, hotkey_id, HOT_KEY_MODIFIERS(modifiers as u32), vk).is_err() {
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == hotkey_id {
                callback();
            }
        }

        let _ = UnregisterHotKey(None, hotkey_id);
    });

    // Spin-wait for the thread to publish its ID (typically < 1ms).
    while thread_id.load(std::sync::atomic::Ordering::Acquire) == 0 {
        std::hint::spin_loop();
    }

    Ok(HotkeyHandle {
        thread_id: thread_id.load(std::sync::atomic::Ordering::Acquire),
        hotkey_id: id,
        thread: Some(thread),
    })
}

#[cfg(not(windows))]
pub fn register_hotkey<F>(_id: i32, _modifiers: u32, _vk: u32, _callback: F) -> Result<HotkeyHandle>
where
    F: Fn() + Send + 'static,
{
    Err(crate::WinScreenError::UnsupportedPlatform)
}

pub fn set_process_dpi_aware() -> Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };

        let ok =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if ok.is_err() {
            tracing::debug!("SetProcessDpiAwarenessContext did not apply; continuing");
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn virtual_screen_rect() -> Result<Rect> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };

        return Ok(Rect {
            x: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            y: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as u32,
            height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) } as u32,
        });
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        Err(WinScreenError::UnsupportedPlatform)
    }
}
