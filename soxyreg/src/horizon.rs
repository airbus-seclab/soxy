use std::io;

/// Path of the `VMware` `Horizon Client` own list of virtual channel DLLs,
/// relative to `HKLM`.
///
/// This is *not* the mstsc/RDS `AddIn` key (`svc::RDP_ADDINS_PATH`): the
/// `Horizon Client` only reads that one when the display protocol is Microsoft
/// RDP. With `PCoIP` or `Blast Extreme` nothing loads `mstscax.dll`, so
/// `soxyreg svc register` has no effect at all there.
const VDM_CLIENT_PATH: &str = "Software\\VMware, Inc.\\VMware VDM\\Client";

/// Name of the `REG_SZ` value holding the comma separated list of
/// `DLL path[,{GUID}]` entries.
const VIRTUAL_CHANNEL_DLLS: &str = "VirtualChannelDLLs";

const ACCESS: u32 = winreg::enums::KEY_ALL_ACCESS;

/// The two registry views have to be updated separately: a 32 bits process
/// writing `HKLM\SOFTWARE\...` is silently redirected to `WOW6432Node`, and the
/// `Horizon Client` ships both a 32 bits launcher (`vmware-view.exe`) and a 64
/// bits plugin host (`vmware-remotemks.exe`). Using the explicit `KEY_WOW64_*`
/// flags makes the intent independent of the bitness of *this* process.
const VIEWS: [(u32, &str); 2] = [
    (winreg::enums::KEY_WOW64_64KEY, "64-bit"),
    (winreg::enums::KEY_WOW64_32KEY, "32-bit"),
];

/// Splits the `VirtualChannelDLLs` value into its entries.
///
/// Each entry is a `DLL path` optionally followed by a `,{GUID}`, so the value
/// is a flat comma separated list where a `{...}` token belongs to the entry
/// preceding it. Tokens are kept verbatim so that the value can be rebuilt
/// without reformatting what was already there.
fn split_entries(value: &str) -> Vec<Vec<String>> {
    let mut entries: Vec<Vec<String>> = Vec::new();

    for token in value.split(',') {
        if token.is_empty() {
            continue;
        }

        let trimmed = token.trim();
        let is_guid = trimmed.starts_with('{') && trimmed.ends_with('}');

        if is_guid && let Some(last) = entries.last_mut() {
            last.push(token.to_string());
        } else {
            entries.push(vec![token.to_string()]);
        }
    }

    entries
}

fn join_entries(entries: &[Vec<String>]) -> String {
    entries
        .iter()
        .map(|entry| entry.join(","))
        .collect::<Vec<_>>()
        .join(",")
}

fn is_our_entry(entry: &[String], dll_path: &str) -> bool {
    entry
        .first()
        .is_some_and(|path| path.trim().eq_ignore_ascii_case(dll_path))
}

/// `ERROR_ACCESS_DENIED`: the `VirtualChannelDLLs` value lives under `HKLM`, so
/// an unelevated process cannot write it.
fn describe(e: &io::Error, view: &str) -> String {
    if e.raw_os_error() == Some(5) {
        format!(
            "access denied on the {view} view: {e}. \
             `VirtualChannelDLLs` lives under HKLM, rerun as Administrator"
        )
    } else {
        format!("failed on the {view} view: {e}")
    }
}

/// Opens (creating it if needed) the Horizon Client key in one registry view.
fn open_client_key(view_flag: u32, view: &str) -> Result<winreg::RegKey, String> {
    let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
    let flags = ACCESS | view_flag;

    // The key can legitimately be absent: a client which never had a virtual
    // channel plugin installed simply does not have it yet.
    hklm.open_subkey_with_flags(VDM_CLIENT_PATH, flags)
        .map_or_else(
            |_| {
                hklm.create_subkey_with_flags(VDM_CLIENT_PATH, flags)
                    .map(|(client, _disp)| client)
                    .map_err(|e| describe(&e, view))
            },
            Ok,
        )
}

/// Appends `dll_path` to `VirtualChannelDLLs` in one registry view.
///
/// Returns whether the value was actually modified.
fn register_view(dll_path: &str, view_flag: u32, view: &str) -> Result<bool, String> {
    let client = open_client_key(view_flag, view)?;

    let current: String = client
        .get_value(VIRTUAL_CHANNEL_DLLS)
        .unwrap_or_default();

    let mut entries = split_entries(&current);

    if entries.iter().any(|entry| is_our_entry(entry, dll_path)) {
        return Ok(false);
    }

    entries.push(vec![dll_path.to_string()]);
    let updated = join_entries(&entries);

    client
        .set_value(VIRTUAL_CHANNEL_DLLS, &updated)
        .map_err(|e| describe(&e, view))?;

    Ok(true)
}

/// Removes `dll_path` (and its optional `,{GUID}`) from `VirtualChannelDLLs`
/// in one registry view.
///
/// Returns whether the value was actually modified.
fn unregister_view(dll_path: &str, view_flag: u32, view: &str) -> Result<bool, String> {
    let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);

    let client = match hklm.open_subkey_with_flags(VDM_CLIENT_PATH, ACCESS | view_flag) {
        Ok(client) => client,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(describe(&e, view)),
    };

    let current: String = match client.get_value(VIRTUAL_CHANNEL_DLLS) {
        Ok(value) => value,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(describe(&e, view)),
    };

    let entries = split_entries(&current);
    let kept: Vec<Vec<String>> = entries
        .into_iter()
        .filter(|entry| !is_our_entry(entry, dll_path))
        .collect();

    if kept.is_empty() {
        // Do not leave an empty value behind: an empty `VirtualChannelDLLs` is
        // what a client which never had a plugin looks like.
        client
            .delete_value(VIRTUAL_CHANNEL_DLLS)
            .map_err(|e| describe(&e, view))?;
    } else {
        client
            .set_value(VIRTUAL_CHANNEL_DLLS, &join_entries(&kept))
            .map_err(|e| describe(&e, view))?;
    }

    Ok(true)
}

pub fn register(dll_path: &str) {
    println!("Horizon Client virtual channel DLL list:");
    println!("  HKLM\\{VDM_CLIENT_PATH}");
    println!("    {VIRTUAL_CHANNEL_DLLS} (REG_SZ)");
    println!();
    println!(
        "Note: the architecture must match `vmware-remotemks.exe` (the process \
         which actually loads the plugins), not `vmware-view.exe`. It is 64-bit \
         even when the Horizon Client is installed as 32-bit."
    );
    println!();

    for (view_flag, view) in VIEWS {
        match register_view(dll_path, view_flag, view) {
            Ok(true) => println!("{view} view: added"),
            Ok(false) => println!("{view} view: already present"),
            Err(e) => eprintln!("{view} view: {e}"),
        }
    }

    println!();
    println!("Now completely exit the Horizon Client (system tray included) and");
    println!("reconnect: the list is read when the client starts.");
    println!();
    println!(
        "If soxy then stays silent, check %APPDATA%\\soxy.toml: Windows paths in \
         it must use forward slashes or TOML literal strings, otherwise the \
         whole file fails to parse and the plugin bails out before logging \
         anything."
    );
}

pub fn unregister(dll_path: &str) {
    for (view_flag, view) in VIEWS {
        match unregister_view(dll_path, view_flag, view) {
            Ok(true) => println!("{view} view: removed"),
            Ok(false) => println!("{view} view: not present"),
            Err(e) => eprintln!("{view} view: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_value_yields_no_entry() {
        assert_eq!(split_entries("").len(), 0);
    }

    #[test]
    fn plain_entries_round_trip() {
        let value = r"C:\a.dll,C:\b.dll";
        let entries = split_entries(value);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], vec![r"C:\a.dll".to_string()]);
        assert_eq!(entries[1], vec![r"C:\b.dll".to_string()]);
        assert_eq!(join_entries(&entries), value);
    }

    #[test]
    fn guid_belongs_to_the_preceding_entry() {
        // This is the shape of a real value: the ThinPrint entry carries a GUID,
        // ours does not.
        let value = r"C:\ThinPrint\TPClnRDP.dll,{DEDC9BA7-C55C-4a3b-965E-B08EAC50D504},C:\soxy.dll";
        let entries = split_entries(value);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].len(), 2);
        assert_eq!(entries[1], vec![r"C:\soxy.dll".to_string()]);
        assert_eq!(join_entries(&entries), value);
    }

    #[test]
    fn our_entry_matches_case_insensitively() {
        let entries = split_entries(r"C:\Soxy.dll");

        assert!(is_our_entry(&entries[0], r"c:\soxy.dll"));
        assert!(!is_our_entry(&entries[0], r"c:\other.dll"));
    }

    #[test]
    fn removing_ours_keeps_the_others() {
        let value = r"C:\ThinPrint\TPClnRDP.dll,{DEDC9BA7-C55C-4a3b-965E-B08EAC50D504},C:\soxy.dll";
        let kept: Vec<Vec<String>> = split_entries(value)
            .into_iter()
            .filter(|entry| !is_our_entry(entry, r"C:\soxy.dll"))
            .collect();

        assert_eq!(
            join_entries(&kept),
            r"C:\ThinPrint\TPClnRDP.dll,{DEDC9BA7-C55C-4a3b-965E-B08EAC50D504}"
        );
    }
}
