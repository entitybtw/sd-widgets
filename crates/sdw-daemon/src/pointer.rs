//! An absolute pointer in the compositor, without pointer acceleration.
//!
//! `ydotool mousemove --absolute` is not absolute on Wayland: it first sends a
//! huge relative reset (`INT32_MIN`) and then the relative distance from the
//! top-left corner. libinput applies mouse acceleration to both, so the cursor
//! lands far from the finger, and a tap can jump across the screen.
//!
//! A small uinput device with `ABS_X`/`ABS_Y` and the three mouse buttons is
//! what libinput calls an absolute mouse: it reports `POINTER_MOTION_ABSOLUTE`
//! and the compositor places the pointer exactly where the event says. No
//! acceleration, no reset, no delta to accumulate. The coordinates are the
//! whole desktop (the compositor's global bounding box), so a multi-monitor
//! stream lands on the right output too.
//!
//! Only Linux is implemented; other platforms keep their native path.

#[cfg(target_os = "linux")]
mod imp {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::mem::size_of;
    use std::os::unix::io::AsRawFd;
    use std::time::Duration;

    const EV_SYN: u16 = 0x00;
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const EV_ABS: u16 = 0x03;
    const SYN_REPORT: u16 = 0x00;
    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;
    const REL_HWHEEL: u16 = 0x06;
    const REL_WHEEL: u16 = 0x08;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    const BTN_MIDDLE: u16 = 0x112;
    const BUS_VIRTUAL: u16 = 0x06;

    // linux/uinput.h. The ioctl numbers are encoded for 64-bit Linux and are
    // part of the kernel ABI, so they are stable.
    const UI_DEV_CREATE: libc::c_ulong = 0x5501;
    const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
    const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
    const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504;
    const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
    const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
    const UI_SET_RELBIT: libc::c_ulong = 0x4004_5566;
    const UI_SET_ABSBIT: libc::c_ulong = 0x4004_5567;

    /// The axis range the kernel will see. 16-bit is plenty for a screen and
    /// keeps the uinput event small.
    const AXIS_MAX: i32 = 65_535;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct InputId {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
    }

    #[repr(C)]
    struct UinputSetup {
        id: InputId,
        name: [u8; 80],
        ff_effects_max: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct InputAbsInfo {
        value: i32,
        minimum: i32,
        maximum: i32,
        fuzz: i32,
        flat: i32,
        resolution: i32,
    }

    #[repr(C)]
    struct UinputAbsSetup {
        code: u16,
        absinfo: InputAbsInfo,
    }

    #[repr(C)]
    struct TimeVal {
        tv_sec: libc::time_t,
        tv_usec: libc::suseconds_t,
    }

    #[repr(C)]
    struct InputEvent {
        time: TimeVal,
        kind: u16,
        code: u16,
        value: i32,
    }

    pub struct AbsolutePointer {
        file: File,
    }

    impl AbsolutePointer {
        /// Create the virtual device. Fails when `/dev/uinput` is not
        /// writable; the caller then keeps the old tool-based path.
        pub fn open() -> Result<Self, String> {
            let file = OpenOptions::new()
                .write(true)
                .open("/dev/uinput")
                .map_err(|e| format!("/dev/uinput: {e}"))?;
            let fd = file.as_raw_fd();
            let set = |req: libc::c_ulong, value: i32| -> Result<(), String> {
                // SAFETY: the fd is a live uinput handle and UI_SET_* consume
                // the integer argument directly.
                let r = unsafe { libc::ioctl(fd, req, value) };
                if r < 0 {
                    Err(format!(
                        "uinput ioctl {req:#x}: {}",
                        std::io::Error::last_os_error()
                    ))
                } else {
                    Ok(())
                }
            };
            let setup_abs = |code: u16| -> Result<(), String> {
                let setup = UinputAbsSetup {
                    code,
                    absinfo: InputAbsInfo {
                        value: 0,
                        minimum: 0,
                        maximum: AXIS_MAX,
                        fuzz: 0,
                        flat: 0,
                        resolution: 1,
                    },
                };
                // SAFETY: UI_ABS_SETUP reads the struct behind the pointer and
                // the struct outlives the call.
                let r = unsafe { libc::ioctl(fd, UI_ABS_SETUP, &setup) };
                if r < 0 {
                    Err(format!(
                        "uinput abs {code}: {}",
                        std::io::Error::last_os_error()
                    ))
                } else {
                    Ok(())
                }
            };

            set(UI_SET_EVBIT, i32::from(EV_KEY))?;
            for key in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
                set(UI_SET_KEYBIT, i32::from(key))?;
            }
            set(UI_SET_EVBIT, i32::from(EV_ABS))?;
            for axis in [ABS_X, ABS_Y] {
                set(UI_SET_ABSBIT, i32::from(axis))?;
                setup_abs(axis)?;
            }
            set(UI_SET_EVBIT, i32::from(EV_REL))?;
            for wheel in [REL_WHEEL, REL_HWHEEL] {
                set(UI_SET_RELBIT, i32::from(wheel))?;
            }

            let mut setup = UinputSetup {
                id: InputId {
                    bustype: BUS_VIRTUAL,
                    vendor: 0x1209,
                    product: 0x53d1,
                    version: 1,
                },
                name: [0; 80],
                ff_effects_max: 0,
            };
            let name = b"sd-widgets pointer";
            setup.name[..name.len()].copy_from_slice(name);
            // SAFETY: UI_DEV_SETUP reads the struct behind the pointer and the
            // struct outlives the call.
            let r = unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup) };
            if r < 0 {
                return Err(format!("uinput setup: {}", std::io::Error::last_os_error()));
            }
            // SAFETY: the fd is a configured uinput handle; UI_DEV_CREATE
            // takes no pointer argument.
            let r = unsafe { libc::ioctl(fd, UI_DEV_CREATE) };
            if r < 0 {
                return Err(format!(
                    "uinput create: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // Let udev and libinput see the new device before the first event;
            // otherwise a touch right after opening the stream is ignored.
            std::thread::sleep(Duration::from_millis(140));
            Ok(Self { file })
        }

        /// Move to `x`/`y` (0..=1 over the whole desktop) and perform an
        /// action. `amount` is wheel steps for the scroll actions.
        pub fn send(&mut self, x: f32, y: f32, action: &str, amount: i32) -> Result<(), String> {
            let x = (x.clamp(0.0, 1.0) * AXIS_MAX as f32).round() as i32;
            let y = (y.clamp(0.0, 1.0) * AXIS_MAX as f32).round() as i32;
            self.event(EV_ABS, ABS_X, x)?;
            self.event(EV_ABS, ABS_Y, y)?;
            self.sync()?;
            let steps = amount.clamp(1, 20);
            match action {
                "move" => Ok(()),
                "down" => self.button(BTN_LEFT, true),
                "up" => self.button(BTN_LEFT, false),
                "click" => self.click(BTN_LEFT),
                "double_click" => {
                    self.click(BTN_LEFT)?;
                    std::thread::sleep(Duration::from_millis(70));
                    self.click(BTN_LEFT)
                }
                "right_click" => self.click(BTN_RIGHT),
                "middle_click" => self.click(BTN_MIDDLE),
                "scroll_up" => self.wheel(0, steps),
                "scroll_down" => self.wheel(0, -steps),
                "scroll_left" => self.wheel(-steps, 0),
                "scroll_right" => self.wheel(steps, 0),
                _ => Ok(()),
            }
        }

        fn event(&mut self, kind: u16, code: u16, value: i32) -> Result<(), String> {
            let event = InputEvent {
                time: TimeVal {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                kind,
                code,
                value,
            };
            // SAFETY: InputEvent is #[repr(C)] and fully initialized; the
            // slice borrows it only for the duration of the write.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    (&event as *const InputEvent).cast::<u8>(),
                    size_of::<InputEvent>(),
                )
            };
            self.file
                .write_all(bytes)
                .map_err(|e| format!("uinput write: {e}"))
        }

        fn sync(&mut self) -> Result<(), String> {
            self.event(EV_SYN, SYN_REPORT, 0)
        }

        fn button(&mut self, code: u16, down: bool) -> Result<(), String> {
            self.event(EV_KEY, code, i32::from(down))?;
            self.sync()
        }

        fn click(&mut self, code: u16) -> Result<(), String> {
            self.button(code, true)?;
            std::thread::sleep(Duration::from_millis(12));
            self.button(code, false)
        }

        fn wheel(&mut self, horizontal: i32, vertical: i32) -> Result<(), String> {
            if horizontal != 0 {
                self.event(EV_REL, REL_HWHEEL, horizontal)?;
            }
            if vertical != 0 {
                self.event(EV_REL, REL_WHEEL, vertical)?;
            }
            self.sync()
        }
    }

    impl Drop for AbsolutePointer {
        fn drop(&mut self) {
            // SAFETY: the fd is alive for the whole drop; UI_DEV_DESTROY takes
            // no pointer argument. A failure here only means the kernel
            // already removed the device.
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
            }
        }
    }

    #[cfg(test)]
    mod abi {
        #[test]
        fn uinput_structs_match_the_kernel_abi() {
            // The ioctl numbers encode these sizes; a layout change here would
            // silently break every absolute move, so pin them.
            assert_eq!(std::mem::size_of::<super::UinputSetup>(), 92);
            assert_eq!(std::mem::size_of::<super::UinputAbsSetup>(), 28);
            assert_eq!(std::mem::size_of::<super::InputEvent>(), 24);
        }

        #[test]
        #[ignore = "creates a real input device; run by hand, it sends no events"]
        fn the_device_is_created_and_removed() {
            let pointer =
                super::AbsolutePointer::open().expect("/dev/uinput must be writable by the user");
            let devices = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
            assert!(
                devices.contains("sd-widgets pointer"),
                "the kernel did not register the device"
            );
            // The compositor only treats it as a pointer when udev tags it as
            // an absolute mouse: `ABS_X`/`ABS_Y` plus a mouse button.
            let mut tagged = false;
            for entry in std::fs::read_dir("/sys/class/input")
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.path().join("name");
                if std::fs::read_to_string(&name)
                    .map(|n| n.trim() == "sd-widgets pointer")
                    .unwrap_or(false)
                {
                    let id = entry.file_name();
                    let data = format!("/run/udev/data/+input:{}", id.to_string_lossy());
                    let props = std::fs::read_to_string(data).unwrap_or_default();
                    tagged = props.lines().any(|l| l == "E:ID_INPUT_MOUSE=1");
                }
            }
            assert!(tagged, "udev did not mark the device as a mouse");
            drop(pointer);
            std::thread::sleep(std::time::Duration::from_millis(300));
            let devices = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
            assert!(
                !devices.contains("sd-widgets pointer"),
                "the device outlived its handle"
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::AbsolutePointer;

#[cfg(not(target_os = "linux"))]
pub struct AbsolutePointer;

#[cfg(not(target_os = "linux"))]
impl AbsolutePointer {
    pub fn open() -> Result<Self, String> {
        Err("absolute pointer is only implemented on Linux".into())
    }

    pub fn send(&mut self, _x: f32, _y: f32, _action: &str, _amount: i32) -> Result<(), String> {
        Err("absolute pointer is only implemented on Linux".into())
    }
}
