//! Temporary raw terminal input for the CLI transcription chooser.

const DEFAULT_DIMENSIONS: (usize, usize) = (24, 100);

#[cfg(any(unix, windows))]
fn require_terminals() -> std::io::Result<()> {
    use std::io::IsTerminal;

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "The transcription chooser requires interactive stdin and stderr",
        ))
    }
}

#[cfg(any(unix, windows))]
fn wait_milliseconds(duration: std::time::Duration, maximum: u32) -> u32 {
    let partial_millisecond = !duration.subsec_nanos().is_multiple_of(1_000_000);
    duration
        .as_millis()
        .saturating_add(u128::from(partial_millisecond))
        .min(u128::from(maximum)) as u32
}

#[cfg(unix)]
mod platform {
    use crate::cli_transcription_choice::KeyInput;
    use std::io;
    use std::mem::MaybeUninit;
    use std::time::Duration;
    use std::time::Instant;

    pub(crate) struct Terminal {
        input_fd: libc::c_int,
        original: libc::termios,
    }

    fn set_attributes(fd: libc::c_int, attributes: &libc::termios) -> io::Result<()> {
        loop {
            // The descriptor is borrowed, and attributes remains valid for the call.
            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, attributes) } == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    impl Terminal {
        pub(crate) fn enter() -> io::Result<Self> {
            super::require_terminals()?;
            Self::enter_fd(libc::STDIN_FILENO)
        }

        fn enter_fd(input_fd: libc::c_int) -> io::Result<Self> {
            let mut original = MaybeUninit::<libc::termios>::uninit();
            // tcgetattr initializes the complete termios value on success.
            if unsafe { libc::tcgetattr(input_fd, original.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let original = unsafe { original.assume_init() };
            let mut raw = original;
            unsafe { libc::cfmakeraw(&mut raw) };
            // The renderer writes normal newlines to stderr on this same terminal.
            raw.c_oflag = original.c_oflag;
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            let terminal = Self { input_fd, original };
            set_attributes(input_fd, &raw)?;
            Ok(terminal)
        }

        fn wait_readable(&self, timeout: Duration) -> io::Result<bool> {
            let started = Instant::now();
            loop {
                let remaining = timeout.saturating_sub(started.elapsed());
                let mut descriptor = libc::pollfd {
                    fd: self.input_fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let milliseconds = super::wait_milliseconds(remaining, i32::MAX as u32);
                let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds as i32) };
                if result > 0 {
                    if descriptor.revents & libc::POLLNVAL != 0 {
                        return Err(io::Error::from_raw_os_error(libc::EBADF));
                    }
                    if descriptor.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                        return Ok(true);
                    }
                } else if result < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
                if started.elapsed() >= timeout {
                    return Ok(false);
                }
            }
        }
    }

    impl KeyInput for Terminal {
        fn read_byte(&mut self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
            let started = Instant::now();
            loop {
                if let Some(timeout) = timeout {
                    if !self.wait_readable(timeout.saturating_sub(started.elapsed()))? {
                        return Ok(None);
                    }
                }
                let mut byte = 0_u8;
                // Read directly, so a buffered stdin reader cannot hide escape-sequence bytes
                // from the next readiness check.
                let count = unsafe {
                    libc::read(self.input_fd, std::ptr::from_mut(&mut byte).cast(), 1)
                };
                if count == 1 {
                    return Ok(Some(byte));
                }
                if count == 0 {
                    return Ok(None);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
                if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
                    return Ok(None);
                }
            }
        }

        fn dimensions(&self) -> (usize, usize) {
            let mut size = MaybeUninit::<libc::winsize>::zeroed();
            let result = unsafe {
                libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr())
            };
            if result != 0 {
                return super::DEFAULT_DIMENSIONS;
            }
            let size = unsafe { size.assume_init() };
            let (default_rows, default_columns) = super::DEFAULT_DIMENSIONS;
            let rows = usize::from(size.ws_row);
            let columns = usize::from(size.ws_col);
            (
                if rows == 0 { default_rows } else { rows },
                if columns == 0 { default_columns } else { columns },
            )
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            let _ = set_attributes(self.input_fd, &self.original);
        }
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    mod tests {
        use super::Terminal;
        use crate::cli_transcription_choice::KeyInput;
        use std::io;
        use std::mem::MaybeUninit;
        use std::os::fd::AsRawFd;
        use std::os::fd::FromRawFd;
        use std::os::fd::OwnedFd;
        use std::time::Duration;

        fn terminal_pair() -> (OwnedFd, OwnedFd) {
            let mut master = -1;
            let mut slave = -1;
            let result = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            assert_eq!(result, 0, "{}", io::Error::last_os_error());
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) }
        }

        fn attributes(fd: &OwnedFd) -> libc::termios {
            let mut attributes = MaybeUninit::<libc::termios>::uninit();
            assert_eq!(
                unsafe { libc::tcgetattr(fd.as_raw_fd(), attributes.as_mut_ptr()) },
                0
            );
            unsafe { attributes.assume_init() }
        }

        #[test]
        fn integration_raw_terminal_restores_the_private_pty_after_an_error() {
            let (_master, slave) = terminal_pair();
            let original = attributes(&slave);
            let result: io::Result<()> = (|| {
                let _terminal = Terminal::enter_fd(slave.as_raw_fd())?;
                let raw = attributes(&slave);
                assert_eq!(raw.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG), 0);
                assert_eq!(raw.c_oflag, original.c_oflag);
                assert_eq!(raw.c_cc[libc::VMIN], 1);
                assert_eq!(raw.c_cc[libc::VTIME], 0);
                Err(io::Error::other("synthetic chooser error"))
            })();
            assert!(result.is_err());
            let restored = attributes(&slave);
            assert_eq!(restored.c_iflag, original.c_iflag);
            assert_eq!(restored.c_oflag, original.c_oflag);
            assert_eq!(restored.c_cflag, original.c_cflag);
            assert_eq!(restored.c_lflag, original.c_lflag);
            assert_eq!(restored.c_cc, original.c_cc);
        }

        #[test]
        fn integration_private_pty_reads_escape_bytes_without_buffered_readiness_gaps() {
            let (master, slave) = terminal_pair();
            let mut terminal = Terminal::enter_fd(slave.as_raw_fd()).unwrap();
            assert_eq!(terminal.read_byte(Some(Duration::ZERO)).unwrap(), None);
            let bytes = b"\x1b[A";
            assert_eq!(
                unsafe { libc::write(master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) },
                bytes.len() as isize
            );
            for byte in bytes {
                assert_eq!(
                    terminal.read_byte(Some(Duration::from_secs(1))).unwrap(),
                    Some(*byte)
                );
            }
            assert_eq!(terminal.read_byte(Some(Duration::ZERO)).unwrap(), None);
        }
    }
}

#[cfg(windows)]
mod platform {
    use crate::cli_transcription_choice::KeyInput;
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::io;
    use std::mem::MaybeUninit;
    use std::time::Duration;
    use std::time::Instant;

    type Handle = *mut c_void;
    const STD_INPUT_HANDLE: u32 = (-10_i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12_i32) as u32;
    const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
    const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;
    const DISABLED_INPUT_FLAGS: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0010 | 0x0040;
    const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;
    const KEY_EVENT: u16 = 1;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }

    #[repr(C)]
    struct ConsoleScreenBufferInfo {
        _size: Coord,
        _cursor_position: Coord,
        _attributes: u16,
        window: SmallRect,
        _maximum_window_size: Coord,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyEventRecord {
        key_down: i32,
        repeat_count: u16,
        virtual_key_code: u16,
        _virtual_scan_code: u16,
        unicode_char: u16,
        control_key_state: u32,
    }

    #[repr(C)]
    union InputEvent {
        key: KeyEventRecord,
        _padding: [u32; 4],
    }

    #[repr(C)]
    struct InputRecord {
        event_type: u16,
        event: InputEvent,
    }

    // Match the Win32 ABI, including the four-byte-aligned INPUT_RECORD union.
    const _: [(); 16] = [(); std::mem::size_of::<KeyEventRecord>()];
    const _: [(); 20] = [(); std::mem::size_of::<InputRecord>()];
    const _: [(); 4] = [(); std::mem::align_of::<InputRecord>()];
    const _: [(); 22] = [(); std::mem::size_of::<ConsoleScreenBufferInfo>()];

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn ReadConsoleInputW(
            handle: Handle,
            records: *mut InputRecord,
            count: u32,
            count_read: *mut u32,
        ) -> i32;
        fn GetConsoleScreenBufferInfo(handle: Handle, info: *mut ConsoleScreenBufferInfo) -> i32;
    }

    pub(crate) struct Terminal {
        input: Handle,
        output: Handle,
        original_input: u32,
        original_output: u32,
        pending: VecDeque<u8>,
    }

    fn console(which: u32) -> io::Result<(Handle, u32)> {
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == (-1_isize) as Handle {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "The standard console handle is unavailable",
            ));
        }
        let mut mode = 0;
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((handle, mode))
    }

    impl Terminal {
        pub(crate) fn enter() -> io::Result<Self> {
            super::require_terminals()?;
            let (input, original_input) = console(STD_INPUT_HANDLE)?;
            let (output, original_output) = console(STD_ERROR_HANDLE)?;
            let output_mode =
                original_output | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
            if unsafe { SetConsoleMode(output, output_mode) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let input_mode = (original_input
                | ENABLE_VIRTUAL_TERMINAL_INPUT
                | ENABLE_EXTENDED_FLAGS)
                & !DISABLED_INPUT_FLAGS;
            if unsafe { SetConsoleMode(input, input_mode) } == 0 {
                let error = io::Error::last_os_error();
                unsafe {
                    SetConsoleMode(input, original_input);
                    SetConsoleMode(output, original_output);
                }
                return Err(error);
            }
            Ok(Self {
                input,
                output,
                original_input,
                original_output,
                pending: VecDeque::new(),
            })
        }

        fn enqueue_key(&mut self, key: KeyEventRecord) {
            if key.key_down == 0 {
                return;
            }
            let sequence: &[u8] = match key.virtual_key_code {
                0x26 => b"\x1b[A",
                0x28 => b"\x1b[B",
                0x27 => b"\x1b[C",
                0x25 => b"\x1b[D",
                0x24 => b"\x1b[H",
                0x23 => b"\x1b[F",
                _ => {
                    let byte = if key.control_key_state & (0x0004 | 0x0008) != 0 {
                        match key.virtual_key_code {
                            0x43 => Some(3),
                            0x44 => Some(4),
                            _ => u8::try_from(key.unicode_char).ok(),
                        }
                    } else {
                        u8::try_from(key.unicode_char).ok()
                    };
                    if let Some(byte @ 1..=127) = byte {
                        let repetitions = usize::from(key.repeat_count.max(1));
                        self.pending.extend(std::iter::repeat_n(byte, repetitions));
                    }
                    return;
                }
            };
            for _ in 0..key.repeat_count.max(1) {
                self.pending.extend(sequence);
            }
        }
    }

    impl KeyInput for Terminal {
        fn read_byte(&mut self, timeout: Option<Duration>) -> io::Result<Option<u8>> {
            if let Some(byte) = self.pending.pop_front() {
                return Ok(Some(byte));
            }
            let started = Instant::now();
            let mut checked_input = false;
            loop {
                if let Some(timeout) = timeout {
                    if checked_input && started.elapsed() >= timeout {
                        return Ok(None);
                    }
                    let remaining = timeout.saturating_sub(started.elapsed());
                    let milliseconds = super::wait_milliseconds(remaining, u32::MAX - 1);
                    match unsafe { WaitForSingleObject(self.input, milliseconds) } {
                        WAIT_OBJECT_0 => {}
                        WAIT_TIMEOUT if started.elapsed() < timeout => continue,
                        WAIT_TIMEOUT => return Ok(None),
                        WAIT_FAILED => return Err(io::Error::last_os_error()),
                        _ => return Err(io::Error::other("Unexpected console input wait result")),
                    }
                }
                checked_input = true;
                let mut record = MaybeUninit::<InputRecord>::zeroed();
                let mut count_read = 0;
                if unsafe { ReadConsoleInputW(self.input, record.as_mut_ptr(), 1, &mut count_read) }
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
                if count_read == 0 {
                    return Ok(None);
                }
                let record = unsafe { record.assume_init() };
                if record.event_type == KEY_EVENT {
                    self.enqueue_key(unsafe { record.event.key });
                    if let Some(byte) = self.pending.pop_front() {
                        return Ok(Some(byte));
                    }
                }
            }
        }

        fn dimensions(&self) -> (usize, usize) {
            let mut info = MaybeUninit::<ConsoleScreenBufferInfo>::uninit();
            if unsafe { GetConsoleScreenBufferInfo(self.output, info.as_mut_ptr()) } == 0 {
                return super::DEFAULT_DIMENSIONS;
            }
            let window = unsafe { info.assume_init() }.window;
            let rows = i32::from(window.bottom) - i32::from(window.top) + 1;
            let columns = i32::from(window.right) - i32::from(window.left) + 1;
            if rows <= 0 || columns <= 0 {
                return super::DEFAULT_DIMENSIONS;
            }
            (rows as usize, columns as usize)
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.input, self.original_input);
                SetConsoleMode(self.output, self.original_output);
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use crate::cli_transcription_choice::KeyInput;
    use std::io;
    use std::time::Duration;

    pub(crate) struct Terminal;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Interactive transcription selection is unsupported on this operating system",
        )
    }

    impl Terminal {
        pub(crate) fn enter() -> io::Result<Self> {
            Err(unsupported())
        }
    }

    impl KeyInput for Terminal {
        fn read_byte(&mut self, _timeout: Option<Duration>) -> io::Result<Option<u8>> {
            Err(unsupported())
        }

        fn dimensions(&self) -> (usize, usize) {
            super::DEFAULT_DIMENSIONS
        }
    }
}

pub(crate) use platform::Terminal;
