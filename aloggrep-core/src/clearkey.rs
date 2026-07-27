//! Ctrl-L 清屏支持（`--hdc` / `--adb` 实时抓取模式使用）。
//!
//! Unix 下把 stdin 切到 cbreak 模式：关闭 `ICANON`（行缓冲）和 `ECHO`，
//! 保留 `ISIG`，所以 Ctrl+C 仍由内核转换成 `SIGINT`，现有的实时抓取
//! 中断逻辑不受影响。切换后逐字节读 stdin，通过 channel 上报给
//! `KeypressGate`，由它在两条日志之间检查并分发。非 Unix 平台整体是
//! 空实现：`CbreakGuard::enable()` 直接成功但什么也不做，
//! `spawn_key_listener()` 返回一个永远收不到数据的 channel。

use std::io;
use std::sync::mpsc::{self, Receiver};

/// 写入 fd 1 用于清屏 + 光标归位的字节序列，不清 scrollback。
pub const CLEAR_SCREEN: &[u8] = b"\x1b[H\x1b[2J";

/// 一个立即失效的 receiver：`try_recv()` 恒返回 `Err`。用于监听器无法启用
/// 时（非 tty，或非 Unix 平台）。
pub fn disabled_listener() -> Receiver<u8> {
    let (_tx, rx) = mpsc::channel();
    rx
}

/// 直接把 [`CLEAR_SCREEN`] 写到 fd 1，绕过 `io::Stdout` 的内部锁——
/// 实时抓取写循环在运行期间一直持有那把锁，同线程重复获取会死锁。
/// 忽略错误：清屏失败不是致命问题。
#[cfg(unix)]
pub fn write_clear_screen() {
    unsafe {
        libc::write(
            libc::STDOUT_FILENO,
            CLEAR_SCREEN.as_ptr() as *const libc::c_void,
            CLEAR_SCREEN.len(),
        );
    }
}

#[cfg(not(unix))]
pub fn write_clear_screen() {}

/// RAII guard：创建时把 stdin 切到 cbreak 模式，`Drop` 时恢复原始 termios。
#[cfg(unix)]
pub struct CbreakGuard {
    original: libc::termios,
}

#[cfg(unix)]
impl CbreakGuard {
    pub fn enable() -> io::Result<Self> {
        use std::mem::MaybeUninit;
        unsafe {
            let mut original = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let original = original.assume_init();

            let mut cbreak = original;
            cbreak.c_lflag &= !(libc::ICANON | libc::ECHO);
            cbreak.c_cc[libc::VMIN] = 1;
            cbreak.c_cc[libc::VTIME] = 0;

            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &cbreak) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(CbreakGuard { original })
        }
    }
}

#[cfg(unix)]
impl Drop for CbreakGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(not(unix))]
pub struct CbreakGuard;

#[cfg(not(unix))]
impl CbreakGuard {
    pub fn enable() -> io::Result<Self> {
        Ok(CbreakGuard)
    }
}

/// 启动后台线程，逐字节阻塞读 stdin 并转发到返回的 channel。
/// 只应在 `CbreakGuard` 生效期间运行（cbreak 模式下单字节读会立即返回，
/// 不必等换行）。线程不 join：读到 EOF/出错就退出，进程结束时也会被回收。
#[cfg(unix)]
pub fn spawn_key_listener() -> Receiver<u8> {
    use std::io::Read;
    use std::thread;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 1];
        let mut stdin = io::stdin();
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    if tx.send(buf[0]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

#[cfg(not(unix))]
pub fn spawn_key_listener() -> Receiver<u8> {
    disabled_listener()
}

/// 包装一个 `io::Result<String>` 行迭代器；每次取下一行之前，先排空
/// `key_rx` 里所有待处理字节，逐个交给 `on_key` 处理。
pub struct KeypressGate<I, F> {
    inner: I,
    key_rx: Receiver<u8>,
    on_key: F,
}

impl<I, F: FnMut(u8)> KeypressGate<I, F> {
    pub fn new(inner: I, key_rx: Receiver<u8>, on_key: F) -> Self {
        KeypressGate { inner, key_rx, on_key }
    }
}

impl<I, F> Iterator for KeypressGate<I, F>
where
    I: Iterator<Item = io::Result<String>>,
    F: FnMut(u8),
{
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Ok(byte) = self.key_rx.try_recv() {
            (self.on_key)(byte);
        }
        self.inner.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn test_clear_screen_sequence() {
        assert_eq!(CLEAR_SCREEN, b"\x1b[H\x1b[2J");
    }

    #[test]
    fn test_disabled_listener_never_yields() {
        let rx = disabled_listener();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_keypress_gate_dispatches_pending_bytes_before_next_line() {
        let lines: Vec<io::Result<String>> =
            vec![Ok("first".to_string()), Ok("second".to_string())];
        let (tx, rx) = mpsc::channel();
        tx.send(0x0Cu8).unwrap();
        tx.send(b'x').unwrap();

        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen_clone = std::rc::Rc::clone(&seen);
        let mut gate = KeypressGate::new(lines.into_iter(), rx, move |b| seen_clone.borrow_mut().push(b));

        assert_eq!(gate.next().unwrap().unwrap(), "first");
        assert_eq!(*seen.borrow(), vec![0x0C, b'x']);

        assert_eq!(gate.next().unwrap().unwrap(), "second");
        assert_eq!(seen.borrow().len(), 2, "no further bytes were pending, handler must not run again");

        assert!(gate.next().is_none());
    }

    #[test]
    fn test_keypress_gate_passes_through_unchanged_when_no_keys_pending() {
        let lines: Vec<io::Result<String>> = vec![Ok("only".to_string())];
        let (_tx, rx) = mpsc::channel();
        let mut calls = 0;
        let mut gate = KeypressGate::new(lines.into_iter(), rx, |_| calls += 1);
        assert_eq!(gate.next().unwrap().unwrap(), "only");
        assert_eq!(calls, 0);
    }
}
