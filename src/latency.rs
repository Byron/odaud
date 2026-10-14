//! Inline controls for comparing a single output with the configured aggregate.

use crate::hal;
use crossterm::{
    cursor::{Hide, MoveToColumn, MoveUp, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    terminal::{self, Clear, ClearType},
};
use std::{
    io::{self, IsTerminal, Write},
    time::Duration,
};

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const O_NONBLOCK: i32 = 4;

#[derive(Debug, PartialEq)]
pub enum Action {
    Redraw,
    Switch(Option<usize>),
    Adjust { output: usize, delta: f64 },
    Save,
    Quit,
}

#[derive(Clone, Copy)]
enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Save,
    Quit,
}

#[derive(Default)]
struct Selection {
    highlighted: usize,
    editing: bool,
}

impl Selection {
    fn key(&mut self, key: Key, count: usize) -> Option<Action> {
        match key {
            Key::Quit => return Some(Action::Quit),
            Key::Save => return Some(Action::Save),
            _ => {}
        }
        if count == 0 {
            return None;
        }
        self.highlighted = self.highlighted.min(count - 1);
        if self.editing {
            let delta = match key {
                Key::Up => 1.0,
                Key::Down => -1.0,
                Key::Left => -10.0,
                Key::Right => 10.0,
                Key::Escape => {
                    self.editing = false;
                    return Some(Action::Switch(None));
                }
                _ => return None,
            };
            Some(Action::Adjust {
                output: self.highlighted,
                delta,
            })
        } else {
            match key {
                Key::Up => self.highlighted = self.highlighted.saturating_sub(1),
                Key::Down => self.highlighted = (self.highlighted + 1).min(count - 1),
                Key::Enter => {
                    self.editing = true;
                    return Some(Action::Switch(Some(self.highlighted)));
                }
                Key::Escape => return Some(Action::Quit),
                _ => return None,
            }
            Some(Action::Redraw)
        }
    }
}

pub struct Ui {
    selection: Selection,
    rows: u16,
    size: (u16, u16),
    restore_raw_mode: bool,
    stdin_flags: i32,
}

impl Ui {
    pub fn new() -> Result<Self, String> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(
                "--latency-check requires an interactive terminal for stdin and stdout".into(),
            );
        }
        let size = terminal::size().map_err(|e| e.to_string())?;
        let stdin_flags = unsafe { fcntl(0, F_GETFL) };
        if stdin_flags == -1 {
            return Err(io::Error::last_os_error().to_string());
        }
        let restore_raw_mode = !terminal::is_raw_mode_enabled().map_err(|e| e.to_string())?;
        if restore_raw_mode {
            terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        }
        let ui = Self {
            selection: Selection::default(),
            rows: 0,
            size,
            restore_raw_mode,
            stdin_flags,
        };
        // Crossterm otherwise blocks waiting for the rest of a split escape or UTF-8 sequence.
        if unsafe { fcntl(0, F_SETFL, stdin_flags | O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error().to_string());
        }
        execute!(io::stdout(), Hide).map_err(|e| e.to_string())?;
        Ok(ui)
    }

    pub fn poll(&mut self, count: usize) -> Result<Option<Action>, String> {
        if !event::poll(Duration::ZERO).map_err(|e| e.to_string())? {
            return Ok(None);
        }
        let key = match event::read().map_err(|e| e.to_string())? {
            Event::Resize(_, _) => return Ok(Some(Action::Redraw)),
            Event::Key(key) if key.kind != KeyEventKind::Release => key,
            _ => return Ok(None),
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok((key.code == KeyCode::Char('c')).then_some(Action::Quit));
        }
        let key = match key.code {
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Escape,
            KeyCode::Char('s' | 'S') => Key::Save,
            KeyCode::Char('q' | 'Q') => Key::Quit,
            _ => return Ok(None),
        };
        Ok(self.selection.key(key, count))
    }

    pub fn draw(
        &mut self,
        devices: &[hal::Device],
        delays: &[f64],
        message: &str,
    ) -> Result<(), String> {
        if devices.is_empty() || devices.len() != delays.len() {
            return Err("latency controls require matching nonempty device and delay lists".into());
        }
        self.selection.highlighted = self.selection.highlighted.min(devices.len() - 1);
        self.clear()?;
        let index = self.selection.highlighted;
        let mut lines = if self.selection.editing {
            vec![
                format!("{:.3} ms | {}", delays[index], devices[index].name),
                format!("Comparing all {} enabled outputs", devices.len()),
                format!("UID: {}", devices[index].uid),
                "Up +1 / Down -1 ms | Left -10 / Right +10 ms".into(),
                "Esc: reference/menu | s: save | q/Ctrl-C: quit".into(),
            ]
        } else {
            let count = usize::from(self.size.1.saturating_sub(6))
                .clamp(1, 8)
                .min(devices.len());
            let first = index.saturating_sub(count / 2).min(devices.len() - count);
            let mut lines = vec![format!("Reference only: {}", devices[0].name)];
            for position in first..first + count {
                lines.push(format!(
                    "{} {}. {:8.3} ms | {}{}",
                    if position == index { ">" } else { " " },
                    position + 1,
                    delays[position],
                    devices[position].name,
                    if position == 0 { " [reference]" } else { "" },
                ));
            }
            lines.extend([
                format!("UID: {}", devices[index].uid),
                "Up/Down: choose | Enter: compare all".into(),
                "s: save | q/Esc/Ctrl-C: quit".into(),
            ]);
            lines
        };
        if !message.is_empty() {
            lines.push(message.into());
        }
        lines.truncate(usize::from(self.size.1.saturating_sub(1).max(1)));
        let columns = usize::from(self.size.0.saturating_sub(1));
        let frame = lines
            .iter()
            .map(|line| terminal_text(line, columns))
            .collect::<Vec<_>>()
            .join("\r\n");
        self.rows = lines.len() as u16;
        let mut output = io::stdout().lock();
        output
            .write_all(frame.as_bytes())
            .and_then(|()| output.flush())
            .map_err(|e| e.to_string())
    }

    pub fn clear(&mut self) -> Result<(), String> {
        let size = terminal::size().map_err(|e| e.to_string())?;
        let mut output = io::stdout().lock();
        if size != self.size {
            // Terminal reflow invalidates old row coordinates; leave that frame in scrollback.
            if self.rows != 0 {
                output.write_all(b"\r\n").map_err(|e| e.to_string())?;
            }
            self.rows = 0;
            self.size = size;
        }
        queue!(output, MoveToColumn(0)).map_err(|e| e.to_string())?;
        if self.rows != 0 {
            if self.rows > 1 {
                queue!(output, MoveUp(self.rows - 1)).map_err(|e| e.to_string())?;
            }
            queue!(output, Clear(ClearType::FromCursorDown)).map_err(|e| e.to_string())?;
            self.rows = 0;
        }
        output.flush().map_err(|e| e.to_string())
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        if self.restore_raw_mode {
            let _ = terminal::disable_raw_mode();
        }
        unsafe { fcntl(0, F_SETFL, self.stdin_flags) };
        let _ = self.clear();
        let _ = execute!(io::stdout(), Show);
    }
}

fn terminal_text(text: &str, columns: usize) -> String {
    text.chars()
        .flat_map(char::escape_default)
        .take(columns)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_switching_and_adjustments_stay_separate() {
        let mut selection = Selection::default();
        assert_eq!(selection.key(Key::Up, 3), Some(Action::Redraw));
        assert_eq!(selection.highlighted, 0);
        for _ in 0..4 {
            assert_eq!(selection.key(Key::Down, 3), Some(Action::Redraw));
        }
        assert_eq!(selection.highlighted, 2);
        assert_eq!(selection.key(Key::Right, 3), None);
        assert_eq!(selection.key(Key::Enter, 3), Some(Action::Switch(Some(2))));
        for (key, delta) in [
            (Key::Up, 1.0),
            (Key::Down, -1.0),
            (Key::Left, -10.0),
            (Key::Right, 10.0),
        ] {
            assert_eq!(
                selection.key(key, 3),
                Some(Action::Adjust { output: 2, delta })
            );
        }
        assert_eq!(selection.highlighted, 2);
        assert_eq!(selection.key(Key::Enter, 3), None);
        assert_eq!(selection.key(Key::Escape, 3), Some(Action::Switch(None)));
        assert_eq!(selection.key(Key::Escape, 3), Some(Action::Quit));
    }

    #[test]
    fn save_and_quit_work_in_both_modes_and_empty_lists_do_not_panic() {
        let mut selection = Selection::default();
        for editing in [false, true] {
            selection.editing = editing;
            assert_eq!(selection.key(Key::Save, 2), Some(Action::Save));
            assert_eq!(selection.key(Key::Quit, 2), Some(Action::Quit));
            assert_eq!(selection.key(Key::Down, 0), None);
        }
        selection.highlighted = 9;
        assert_eq!(
            selection.key(Key::Up, 2),
            Some(Action::Adjust {
                output: 1,
                delta: 1.0
            })
        );
    }

    #[test]
    fn labels_cannot_control_the_terminal_or_wrap_rows() {
        assert_eq!(terminal_text("Bose USB", 80), "Bose USB");
        assert_eq!(
            terminal_text("a\nb\u{1b}[2Jé", 80),
            "a\\nb\\u{1b}[2J\\u{e9}"
        );
        assert_eq!(terminal_text("0123456789", 4), "0123");
        assert_eq!(terminal_text("anything", 0), "");
    }

    #[test]
    fn partial_terminal_input_still_allows_shutdown_and_restores_modes() {
        use std::{
            fs::File,
            io::Read,
            os::fd::{AsRawFd, FromRawFd},
            process::{Child, Command},
            sync::atomic::Ordering,
            thread,
            time::Instant,
        };

        const CHILD: &str = "ODAUD_TERMINAL_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            unsafe { crate::signal(15, crate::on_signal as *const () as usize) };
            let mut ui = Ui::new().unwrap();
            println!("terminal-ready\r");
            io::stdout().flush().unwrap();
            while !crate::STOP.load(Ordering::Relaxed) {
                ui.poll(2).unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            drop(ui);
            return;
        }

        unsafe extern "C" {
            fn openpty(
                master: *mut i32,
                slave: *mut i32,
                name: *mut u8,
                termios: *const std::ffi::c_void,
                size: *const u16,
            ) -> i32;
            fn kill(pid: i32, signal: i32) -> i32;
        }
        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let terminal_modes = |slave: &File| {
            let result = Command::new("/bin/stty")
                .arg("-g")
                .stdin(slave.try_clone().unwrap())
                .output()
                .unwrap();
            assert!(result.status.success());
            result.stdout
        };

        for partial in [b"\x1b[".as_slice(), b"\xc3".as_slice()] {
            let (mut master, mut slave) = (-1, -1);
            assert_eq!(
                unsafe {
                    openpty(
                        &mut master,
                        &mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null(),
                        [24, 100, 0, 0].as_ptr(),
                    )
                },
                0
            );
            let (mut master, slave) =
                unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
            let original_modes = terminal_modes(&slave);
            let original_flags = unsafe { fcntl(slave.as_raw_fd(), F_GETFL) };
            assert_ne!(original_flags, -1);
            assert_ne!(
                unsafe { fcntl(master.as_raw_fd(), F_SETFL, O_NONBLOCK) },
                -1
            );
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "latency::tests::partial_terminal_input_still_allows_shutdown_and_restores_modes",
                        "--nocapture",
                    ])
                    .env(CHILD, "1")
                    .stdin(slave.try_clone().unwrap())
                    .stdout(slave.try_clone().unwrap())
                    .stderr(slave.try_clone().unwrap())
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut output = Vec::new();
            while !output
                .windows(b"terminal-ready".len())
                .any(|s| s == b"terminal-ready")
            {
                let mut bytes = [0; 1024];
                match master.read(&mut bytes) {
                    Ok(count) => output.extend_from_slice(&bytes[..count]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("PTY read: {error}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "terminal child did not start: {output:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
            master.write_all(partial).unwrap();
            thread::sleep(Duration::from_millis(100));
            assert_eq!(unsafe { kill(child.0.id() as i32, 15) }, 0);
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                assert!(Instant::now() < deadline, "partial input blocked shutdown");
                thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(terminal_modes(&slave), original_modes);
            assert_eq!(
                unsafe { fcntl(slave.as_raw_fd(), F_GETFL) } & O_NONBLOCK,
                original_flags & O_NONBLOCK
            );
        }
    }
}
