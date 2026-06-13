//! Serial port discovery — enumerate candidate ports and their USB VID/PID (§6 of the design spec).
//!
//! [`find_ports`] lists every candidate serial port with its USB VID/PID; [`find_medius`] filters to
//! the box by CH343 (WCH) vendor and product id. Reconnect rescans by VID/PID, not a fixed path, so a
//! re-enumerated device is found again. OS-specific enumeration (sysfs/SetupAPI) is untestable here,
//! so the parsing is factored into pure, unit-tested helpers.

use crate::types::PortInfo;

/// WCH (Jiangsu Qinheng) USB vendor id — the CH343 USB-serial bridge the medius box uses (§6).
pub(crate) const WCH_VID: u16 = 0x1A86;

/// The CH343 USB product id, confirmed on the medius board hardware (`idProduct = 55d3`).
pub(crate) const CH343_PID: u16 = 0x55D3;

/// Parse a sysfs hex id string (e.g. `"1a86"`) into a `u16`. Tolerates surrounding whitespace, a
/// trailing newline, and an optional `0x` prefix; `None` on invalid hex or `u16` overflow.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_sysfs_hex(s: &str) -> Option<u16> {
    let t = s.trim();
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    if t.is_empty() {
        return None;
    }
    u16::from_str_radix(t, 16).ok()
}

/// Parse a Windows hardware-id string for the USB VID/PID, e.g.
/// `USB\VID_1A86&PID_55D3&...` → `(0x1A86, 0x55D3)`. Case-insensitive; `None` if `VID_` or `PID_`
/// (followed by four hex digits) is absent.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_usb_hardware_id(hwid: &str) -> Option<(u16, u16)> {
    let upper = hwid.to_ascii_uppercase();
    let vid = extract_hex4_after(&upper, "VID_")?;
    let pid = extract_hex4_after(&upper, "PID_")?;
    Some((vid, pid))
}

/// Find `marker` in `upper` (already upper-cased) and parse the four hex digits that follow it.
#[cfg_attr(not(windows), allow(dead_code))]
fn extract_hex4_after(upper: &str, marker: &str) -> Option<u16> {
    let idx = upper.find(marker)? + marker.len();
    let digits: &str = upper.get(idx..idx + 4)?;
    if digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        u16::from_str_radix(digits, 16).ok()
    } else {
        None
    }
}

/// Enumerate candidate serial ports with their USB VID/PID (empty on unsupported targets).
pub(crate) fn find_ports() -> Vec<PortInfo> {
    #[cfg(target_os = "linux")]
    {
        linux_find_ports()
    }
    #[cfg(windows)]
    {
        windows_find_ports()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

/// Discover medius boxes: the candidate serial ports filtered to the WCH vendor id and the CH343
/// product id (§6). The handshake remains the final gate distinguishing a box from any other
/// CH343-based serial device.
pub fn find_medius() -> Vec<PortInfo> {
    find_ports()
        .into_iter()
        .filter(|p| p.vid == WCH_VID && p.pid == CH343_PID)
        .collect()
}

// ---- Linux enumeration (sysfs) ----

#[cfg(target_os = "linux")]
fn linux_find_ports() -> Vec<PortInfo> {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Walk up from a tty's `device` dir to the USB device dir (which holds `idVendor`/`idProduct`)
    /// and read the VID/PID. `device` symlinks to the USB *interface* dir; idVendor lives on an
    /// ancestor.
    fn vid_pid_for(class_dir: &Path) -> Option<(u16, u16)> {
        let start: PathBuf = fs::canonicalize(class_dir.join("device")).ok()?;
        let mut dir: &Path = start.as_path();
        loop {
            let vid_path = dir.join("idVendor");
            if vid_path.exists() {
                let vid = parse_sysfs_hex(&fs::read_to_string(&vid_path).ok()?)?;
                let pid = parse_sysfs_hex(&fs::read_to_string(dir.join("idProduct")).ok()?)?;
                return Some((vid, pid));
            }
            dir = dir.parent()?;
            // Don't escape /sys.
            if dir == Path::new("/") || !dir.starts_with("/sys") {
                return None;
            }
        }
    }

    let mut ports = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/tty") else {
        return ports;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("ttyACM") || name.starts_with("ttyUSB")) {
            continue;
        }
        let class_dir = entry.path();
        // Only USB-backed ttys resolve to an idVendor ancestor.
        if let Some((vid, pid)) = vid_pid_for(&class_dir) {
            ports.push(PortInfo {
                path: format!("/dev/{name}"),
                vid,
                pid,
            });
        }
    }
    ports.sort_by(|a, b| a.path.cmp(&b.path));
    ports
}

// ---- Windows enumeration (SetupAPI) ----

#[cfg(windows)]
fn windows_find_ports() -> Vec<PortInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_PRESENT, GUID_DEVCLASS_PORTS, HDEVINFO, SP_DEVINFO_DATA, SPDRP_FRIENDLYNAME,
        SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
        SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW,
    };
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError};

    /// `INVALID_HANDLE_VALUE` as an `HDEVINFO` (which is an `isize`, not a pointer).
    const INVALID_HDEVINFO: HDEVINFO = -1;

    /// Read a string device property (REG_SZ / REG_MULTI_SZ) for one device.
    ///
    /// # Safety
    /// `hdi` must be a valid device-info-set handle and `data` a populated `SP_DEVINFO_DATA` from
    /// [`SetupDiEnumDeviceInfo`] on the same set.
    unsafe fn read_string_prop(
        hdi: HDEVINFO,
        data: *mut SP_DEVINFO_DATA,
        prop: u32,
    ) -> Option<String> {
        let mut needed: u32 = 0;
        // SAFETY: null buffer + size 0 is the documented size-probe form; `needed` receives the
        // required byte count. `hdi`/`data` valid per this fn's contract.
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                hdi,
                data,
                prop,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 {
            return None;
        }
        // `needed` is only a reliable size if the probe failed with ERROR_INSUFFICIENT_BUFFER; any
        // other failure (e.g. ERROR_INVALID_DATA on a corrupt entry) means bail, don't allocate.
        // SAFETY: GetLastError reads this thread's last-error; no Windows call intervened since the
        // probe.
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        // SAFETY: `buf` is `needed` bytes; the call fills it with a UTF-16 string. `hdi`/`data` valid
        // per this fn's contract.
        let ok = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                hdi,
                data,
                prop,
                core::ptr::null_mut(),
                buf.as_mut_ptr(),
                needed,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        // Reinterpret as u16, up to the first NUL.
        let wide: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_ne_bytes([c[0], c[1]]))
            .take_while(|&w| w != 0)
            .collect();
        Some(OsString::from_wide(&wide).to_string_lossy().into_owned())
    }

    /// Extract the `COMn` token from a friendly name like `"USB-SERIAL CH343 (COM7)"`.
    fn com_from_friendly(name: &str) -> Option<String> {
        let open = name.rfind('(')?;
        let close = name[open..].find(')')? + open;
        let inner = name[open + 1..close].trim();
        if inner.to_ascii_uppercase().starts_with("COM") {
            Some(inner.to_string())
        } else {
            None
        }
    }

    let mut ports = Vec::new();

    // SAFETY: GUID_DEVCLASS_PORTS is a valid class GUID; null enumerator/parent request all present
    // devices of that class. Handle released via SetupDiDestroyDeviceInfoList below.
    let hdi = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVCLASS_PORTS,
            core::ptr::null(),
            core::ptr::null_mut(),
            DIGCF_PRESENT,
        )
    };
    if hdi == INVALID_HDEVINFO {
        return ports;
    }

    let mut index: u32 = 0;
    loop {
        let mut data: SP_DEVINFO_DATA = unsafe { core::mem::zeroed() };
        data.cbSize = core::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        // SAFETY: `hdi` is the valid set from above; `data.cbSize` is initialized as required.
        let ok = unsafe { SetupDiEnumDeviceInfo(hdi, index, &mut data) };
        if ok == 0 {
            break; // no more devices
        }
        index += 1;

        // SAFETY: `hdi`/`data` are valid for this iteration (both calls).
        let hwid = unsafe { read_string_prop(hdi, &mut data, SPDRP_HARDWAREID) };
        let friendly = unsafe { read_string_prop(hdi, &mut data, SPDRP_FRIENDLYNAME) };

        if let (Some(hwid), Some(friendly)) = (hwid, friendly)
            && let (Some((vid, pid)), Some(path)) =
                (parse_usb_hardware_id(&hwid), com_from_friendly(&friendly))
        {
            ports.push(PortInfo { path, vid, pid });
        }
    }

    // SAFETY: `hdi` is the valid handle from SetupDiGetClassDevsW and is not used after this call.
    unsafe {
        SetupDiDestroyDeviceInfoList(hdi);
    }

    ports.sort_by(|a, b| a.path.cmp(&b.path));
    ports
}
