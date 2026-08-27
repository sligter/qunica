//! Windows clipboard-history bridge.
//!
//! Copies made inside WebView2 content never appear in the Windows clipboard
//! history (Win+V): the browser places the data on a clipboard owned by its own
//! hidden window, and the history service only records items owned by a
//! visible top-level window of a registered application. Paste still works
//! everywhere, which is why the gap is invisible until a clipboard manager is
//! used. This is a known WebView2 limitation (MicrosoftEdge/WebView2Feedback
//! #5650); the recommended workaround is to re-set the clipboard data with the
//! host's own window as owner, which is exactly what this module does.
//!
//! Ownership transfer needs the full ceremony — open with the visible window,
//! [`EmptyClipboard`], set the data, close. Skipping `EmptyClipboard` leaves
//! the old owner registered for every other format and the history service
//! keeps attributing the item to the invisible WebView2 window, which is
//! precisely the bug being worked around.
//!
//! A background message-only window listens for clipboard updates. It only
//! mirrors an update when the foreground top-level window belongs to this
//! process. This distinction matters: checking the clipboard owner's PID is
//! unreliable because WebView2 may own the clipboard from a renderer process,
//! while omitting an origin check would rewrite copies made in every other
//! application and discard their HTML/image formats.
//!
//! The update caused by the re-set itself is recognized through the clipboard
//! sequence number. Only plain text is mirrored; rich formats copied from this
//! app are intentionally left behind as part of the WebView2 workaround.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RemoveClipboardFormatListener, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, IsWindowVisible, RegisterClassW, TranslateMessage,
    HWND_MESSAGE, MSG, WM_CLIPBOARDUPDATE, WM_DESTROY, WNDCLASSW,
};

/// Registered clipboard format for plain text (standard Win32 value 13).
///
/// Defined locally because the shared `CF_*` constants live behind a feature
/// flag this shell otherwise has no use for.
const CF_UNICODETEXT: u32 = 13;

/// Grace period before acting on an update, so WebView2 finishes staging every
/// format of the copy before the text is mirrored.
const SETTLE_DELAY: Duration = Duration::from_millis(150);

/// `OpenClipboard` retries while another window holds the clipboard open.
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(120);
const MAX_OPEN_ATTEMPTS: usize = 5;

/// Clipboard sequence number observed right after our own re-set, used to
/// recognize (and stop at) the update the re-set itself raises.
static LAST_OWN_SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// Start the listener thread. Returns immediately.
pub fn start() {
    if let Err(error) = std::thread::Builder::new()
        .name("clipboard-history".to_string())
        .spawn(run_message_loop)
    {
        tracing::warn!(
            target: "qunica::desktop",
            %error,
            "failed to start clipboard history bridge"
        );
    }
}

fn run_message_loop() {
    // SAFETY: standard Win32 message-window bootstrap, following the documented
    // register-class / create-window / message-loop order. A failed step bails
    // out of this thread only, degrading to stock clipboard behaviour.
    unsafe {
        let class_name: Vec<u16> = "QunicaClipboardBridge\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        if instance.is_null() {
            tracing::warn!(
                target: "qunica::desktop",
                "clipboard bridge could not resolve the application module"
            );
            return;
        }
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(bridge_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            tracing::warn!(
                target: "qunica::desktop",
                "clipboard bridge window class registration failed"
            );
            return;
        }
        let hwnd = CreateWindowExW(
            0,                    // extended style: none
            class_name.as_ptr(),  // class
            class_name.as_ptr(),  // window name (never shown)
            0,                    // style: none
            0,                    // x
            0,                    // y
            0,                    // width
            0,                    // height
            HWND_MESSAGE,         // parent: message-only window
            std::ptr::null_mut(), // menu
            instance,             // instance
            std::ptr::null(),     // creation params
        );
        if hwnd.is_null() {
            tracing::warn!(
                target: "qunica::desktop",
                "clipboard bridge window creation failed"
            );
            return;
        }
        if AddClipboardFormatListener(hwnd) == 0 {
            tracing::warn!(
                target: "qunica::desktop",
                "clipboard format listener registration failed"
            );
            DestroyWindow(hwnd);
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn bridge_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match msg {
        WM_CLIPBOARDUPDATE => {
            maybe_reown_copy();
            0
        }
        WM_DESTROY => {
            RemoveClipboardFormatListener(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Re-place a foreign-owned text copy under the main window's ownership.
///
/// Two phases. First the text is snapshotted through a neutral clipboard open;
/// then the clipboard is reopened *by the visible main window*, emptied, and
/// fed the snapshot — `EmptyClipboard` is what retargets the ownership the
/// history service looks at. The sequence number stored afterwards marks the
/// update our own write produced, so the next event stops there instead of
/// mirroring forever.
unsafe fn maybe_reown_copy() {
    // The foreground top-level window identifies where the copy originated.
    // WebView2's clipboard owner may be a hidden renderer window (and even a
    // different PID), so comparing the clipboard owner itself cannot safely
    // distinguish our copy from another application's.
    let owner_window = GetForegroundWindow();
    if owner_window.is_null() || IsWindowVisible(owner_window) == 0 {
        return;
    }
    let mut foreground_pid = 0;
    GetWindowThreadProcessId(owner_window, &mut foreground_pid);
    if foreground_pid != GetCurrentProcessId() {
        return;
    }

    let observed_sequence = GetClipboardSequenceNumber();

    // Our own re-set lands here as a second update. Bail before the grace
    // period so the listener thread stays responsive.
    if observed_sequence == LAST_OWN_SEQUENCE.load(Ordering::Acquire) {
        return;
    }

    std::thread::sleep(SETTLE_DELAY);

    // Focus or clipboard contents may have changed during the grace period.
    // In either case this event no longer proves that the current clipboard
    // came from our webview; a later queued update can make its own decision.
    if GetForegroundWindow() != owner_window || GetClipboardSequenceNumber() != observed_sequence {
        return;
    }

    let Some(text) = read_clipboard_text() else {
        return;
    };
    // Allocate and fill the replacement before emptying the clipboard. If the
    // allocation fails, the user's existing copy remains intact.
    let Some(allocated) = allocate_text(&text) else {
        return;
    };

    for attempt in 1..=MAX_OPEN_ATTEMPTS {
        if OpenClipboard(owner_window) == 0 {
            if attempt == MAX_OPEN_ATTEMPTS {
                GlobalFree(allocated);
                return;
            }
            std::thread::sleep(OPEN_RETRY_DELAY);
            continue;
        }

        // Never replace a newer clipboard item or one copied after focus left
        // this app while the listener was waiting to open the clipboard.
        if GetForegroundWindow() != owner_window
            || GetClipboardSequenceNumber() != observed_sequence
        {
            CloseClipboard();
            GlobalFree(allocated);
            return;
        }

        // The full ceremony: emptying hands every format's ownership to the
        // window we opened with, then the single text format is written back.
        if EmptyClipboard() == 0 {
            CloseClipboard();
            GlobalFree(allocated);
            return;
        }
        if SetClipboardData(CF_UNICODETEXT, allocated).is_null() {
            CloseClipboard();
            // Ownership transfers only on success.
            GlobalFree(allocated);
            return;
        }
        CloseClipboard();
        LAST_OWN_SEQUENCE.store(GetClipboardSequenceNumber(), Ordering::Release);
        return;
    }
}

/// Read the current `CF_UNICODETEXT`, if any, as well-formed UTF-16.
///
/// Opened without a specific owner window: this pass only reads, and claiming
/// ownership here would defeat the re-own below.
unsafe fn read_clipboard_text() -> Option<Vec<u16>> {
    for attempt in 1..=MAX_OPEN_ATTEMPTS {
        if OpenClipboard(std::ptr::null_mut()) != 0 {
            break;
        }
        if attempt == MAX_OPEN_ATTEMPTS {
            return None;
        }
        std::thread::sleep(OPEN_RETRY_DELAY);
    }

    let result = (|| {
        let source = GetClipboardData(CF_UNICODETEXT);
        if source.is_null() {
            return None;
        }
        let byte_size = GlobalSize(source);
        if byte_size < 2 {
            return None;
        }
        let locked = GlobalLock(source);
        if locked.is_null() {
            return None;
        }
        let units = std::slice::from_raw_parts(locked as *const u16, byte_size / 2);
        let text = normalize_utf16(units);
        GlobalUnlock(source);
        (!text.is_empty()).then_some(text)
    })();

    CloseClipboard();
    result
}

/// Allocate a movable, NUL-terminated UTF-16 clipboard payload.
///
/// SAFETY: the returned allocation remains owned by the caller until a
/// successful `SetClipboardData` transfers it to the system.
unsafe fn allocate_text(text: &[u16]) -> Option<*mut core::ffi::c_void> {
    let bytes = (text.len() + 1) * 2;
    let allocated = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if allocated.is_null() {
        return None;
    }
    let target = GlobalLock(allocated);
    if target.is_null() {
        GlobalFree(allocated);
        return None;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr(), target as *mut u16, text.len());
    *(target as *mut u16).add(text.len()) = 0;
    GlobalUnlock(allocated);
    Some(allocated)
}

/// Cut a NUL-terminated UTF-16 buffer down to well-formed UTF-16 text.
///
/// Stops at the terminator and repairs unpaired surrogates (lossy), so a
/// truncated or malformed source can never produce an invalid re-set.
fn normalize_utf16(units: &[u16]) -> Vec<u16> {
    let end = units
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
        .encode_utf16()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_utf16;

    #[test]
    fn stops_at_the_nul_terminator() {
        let units: Vec<u16> = "hi\0hidden".encode_utf16().collect();
        assert_eq!(
            normalize_utf16(&units),
            "hi".encode_utf16().collect::<Vec<u16>>()
        );
    }

    #[test]
    fn preserves_astral_characters_across_the_round_trip() {
        let units: Vec<u16> = "\u{1f980} \u{4e2d}\u{6587}".encode_utf16().collect();
        assert_eq!(
            normalize_utf16(&units),
            "\u{1f980} \u{4e2d}\u{6587}"
                .encode_utf16()
                .collect::<Vec<u16>>()
        );
    }

    #[test]
    fn repairs_unpaired_surrogates_instead_of_panicking() {
        let repaired = normalize_utf16(&[0xD83D]);
        assert_eq!(repaired, vec![0xFFFD]);
    }

    #[test]
    fn an_empty_buffer_yields_no_text() {
        assert!(normalize_utf16(&[]).is_empty());
        assert!(normalize_utf16(&[0]).is_empty());
    }
}
