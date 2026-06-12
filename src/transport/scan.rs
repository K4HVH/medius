//! Serial port discovery — enumerate candidate ports and their USB VID/PID (§6 of the design spec).
//!
//! [`find_ports`] returns every candidate serial port with its USB vendor/product id;
//! [`find_medius`] filters to the medius box by the CH343 (WCH) USB vendor id. Reconnect rescans by
//! VID/PID (not a fixed path) so a re-enumerated device is found again.
//!
//! Enumeration is OS-specific and untestable here (it hits sysfs / SetupAPI), so the **parsing** is
//! factored into pure helpers ([`parse_sysfs_hex`], [`parse_usb_hardware_id`]) that are unit-tested.

/// Information about one discovered serial port.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PortInfo {
    /// The OS path used to open the port (`/dev/ttyACM0` on Linux, `COM3` on Windows).
    pub path: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
}

/// WCH (Jiangsu Qinheng) USB vendor id — the CH343 USB-serial bridge the medius box uses (§6).
pub const WCH_VID: u16 = 0x1A86;

/// The CH343 USB product id observed on the medius board (`idProduct = 55d3`).
// TODO confirm exact PID from board across revisions — discovery matches on VID only for now, so a
// different CH343 variant PID is still found.
// Documents the observed board PID; `find_medius` matches on VID only, so this is reference-only
// (and asserted by `tests::ch343_constants`) rather than used at runtime.
#[allow(dead_code)]
pub const CH343_PID: u16 = 0x55D3;

/// Parse a sysfs hex id string (e.g. `"1a86"` from `idVendor`) into a `u16`.
///
/// Tolerates a trailing newline and surrounding whitespace (sysfs files end with `\n`) and an
/// optional `0x` prefix. Returns `None` if the trimmed string is not valid hex or overflows `u16`.
///
/// Linux-only: only `linux_find_ports` consumes it (it parses sysfs `idVendor`/`idProduct`).
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
/// `USB\VID_1A86&PID_55D3&...` → `(0x1A86, 0x55D3)`.
///
/// Case-insensitive; tolerant of leading text and any trailing `&...` fields. Returns `None` if
/// either `VID_` or `PID_` (followed by four hex digits) is absent.
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

/// Enumerate candidate serial ports with their USB VID/PID.
///
/// On unsupported targets this returns an empty `Vec`.
pub fn find_ports() -> Vec<PortInfo> {
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

/// Discover medius boxes — [`find_ports`] filtered to the WCH vendor id (§6).
///
/// Matches on [`WCH_VID`] only (any CH343 PID), since the exact board PID is not yet pinned across
/// revisions; the named [`CH343_PID`] documents the observed value.
pub fn find_medius() -> Vec<PortInfo> {
    find_ports()
        .into_iter()
        .filter(|p| p.vid == WCH_VID)
        .collect()
}

// ---- Linux enumeration (sysfs) ----

#[cfg(target_os = "linux")]
fn linux_find_ports() -> Vec<PortInfo> {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Walk up from a tty's `device` dir until a directory containing `idVendor` is found (the USB
    /// device dir, one or more levels above the interface dir), and read the VID/PID there.
    fn vid_pid_for(class_dir: &Path) -> Option<(u16, u16)> {
        // `/sys/class/tty/ttyACMx/device` is a symlink to the USB *interface* dir; idVendor lives on
        // an ancestor (the USB device dir). Canonicalize, then walk parents.
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
        // Only real USB-backed ttys have a `device` symlink resolving to an idVendor ancestor.
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

    /// `INVALID_HANDLE_VALUE` as an `HDEVINFO` (which is an `isize`, not a pointer).
    const INVALID_HDEVINFO: HDEVINFO = -1;

    /// Read a string device property (REG_SZ / REG_MULTI_SZ) for one device, as a Rust `String`.
    ///
    /// # Safety
    /// `hdi` must be a valid device-info-set handle and `data` a valid populated `SP_DEVINFO_DATA`
    /// from [`SetupDiEnumDeviceInfo`] on the same set.
    unsafe fn read_string_prop(
        hdi: HDEVINFO,
        data: *mut SP_DEVINFO_DATA,
        prop: u32,
    ) -> Option<String> {
        // First call sizes the buffer.
        let mut needed: u32 = 0;
        // SAFETY: passing a null buffer with size 0 is the documented size-probe form; `needed`
        // receives the required byte count. `hdi`/`data` are valid per this fn's contract.
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
        let mut buf = vec![0u8; needed as usize];
        // SAFETY: `buf` is `needed` bytes; the call fills it with a wide (UTF-16) string. `hdi`/
        // `data` are valid per this fn's contract.
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
        // Reinterpret the byte buffer as u16 and take up to the first NUL.
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
    // devices of that class. The returned handle is released via SetupDiDestroyDeviceInfoList below.
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

        // SAFETY: `hdi`/`data` are valid for this iteration.
        let hwid = unsafe { read_string_prop(hdi, &mut data, SPDRP_HARDWAREID) };
        // SAFETY: same.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sysfs_hex_typical() {
        assert_eq!(parse_sysfs_hex("1a86"), Some(0x1A86));
        assert_eq!(parse_sysfs_hex("55d3"), Some(0x55D3));
        // sysfs files carry a trailing newline.
        assert_eq!(parse_sysfs_hex("1a86\n"), Some(0x1A86));
        assert_eq!(parse_sysfs_hex("  ffff \n"), Some(0xFFFF));
        // Uppercase and 0x prefix tolerated.
        assert_eq!(parse_sysfs_hex("0x1A86"), Some(0x1A86));
        assert_eq!(parse_sysfs_hex("FFFF"), Some(0xFFFF));
        assert_eq!(parse_sysfs_hex("0000"), Some(0x0000));
    }

    #[test]
    fn parse_sysfs_hex_rejects_garbage() {
        assert_eq!(parse_sysfs_hex(""), None);
        assert_eq!(parse_sysfs_hex("   "), None);
        assert_eq!(parse_sysfs_hex("zzzz"), None);
        // Overflows u16.
        assert_eq!(parse_sysfs_hex("10000"), None);
    }

    #[test]
    fn parse_hardware_id_typical() {
        assert_eq!(
            parse_usb_hardware_id(r"USB\VID_1A86&PID_55D3&REV_0100"),
            Some((0x1A86, 0x55D3))
        );
        // Lowercase input.
        assert_eq!(
            parse_usb_hardware_id(r"usb\vid_1a86&pid_55d3"),
            Some((0x1A86, 0x55D3))
        );
        // Multi-field with an interface suffix.
        assert_eq!(
            parse_usb_hardware_id(r"USB\VID_046D&PID_C534&MI_01"),
            Some((0x046D, 0xC534))
        );
    }

    #[test]
    fn parse_hardware_id_missing_fields() {
        assert_eq!(parse_usb_hardware_id(r"USB\VID_1A86"), None); // no PID
        assert_eq!(parse_usb_hardware_id(r"PID_55D3"), None); // no VID
        assert_eq!(parse_usb_hardware_id("nonsense"), None);
        // VID_ marker but fewer than 4 hex digits after it.
        assert_eq!(parse_usb_hardware_id(r"USB\VID_1A&PID_55D3"), None);
        // Non-hex digits after the marker.
        assert_eq!(parse_usb_hardware_id(r"USB\VID_ZZZZ&PID_55D3"), None);
    }

    #[test]
    fn ch343_constants() {
        assert_eq!(WCH_VID, 0x1A86);
        assert_eq!(CH343_PID, 0x55D3);
    }

    /// `find_medius` keeps only WCH-vendor ports (logic test independent of real enumeration).
    #[test]
    fn find_medius_filters_by_vendor() {
        // Direct test of the filter predicate via a synthetic list (mirrors find_medius()).
        let all = vec![
            PortInfo {
                path: "/dev/ttyACM0".into(),
                vid: WCH_VID,
                pid: CH343_PID,
            },
            PortInfo {
                path: "/dev/ttyACM1".into(),
                vid: 0x046D,
                pid: 0xC534,
            },
            PortInfo {
                path: "/dev/ttyUSB0".into(),
                vid: WCH_VID,
                pid: 0x7523,
            },
        ];
        let medius: Vec<_> = all.into_iter().filter(|p| p.vid == WCH_VID).collect();
        assert_eq!(medius.len(), 2);
        assert!(medius.iter().all(|p| p.vid == WCH_VID));
    }

    /// `find_ports` must not panic on this host (it may legitimately return an empty list in CI).
    #[test]
    fn find_ports_does_not_panic() {
        let _ = find_ports();
    }
}
