//! Attachment file names: safe to write anywhere, and honest about their type.

const RESERVED_WINDOWS: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Extensions that run code when opened.
const EXECUTABLE: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "scr",
    "pif", "cpl", "msi", "msp", "lnk", "jar", "reg", "inf", "appref-ms", "application", "gadget",
    "app", "command", "pkg", "dmg", "sh", "workflow", "iso", "img", "vhd", "vhdx", "url", "chm",
];

/// A file name with path separators, control and bidi characters, reserved
/// device names and trailing dots or spaces removed.
pub fn sanitise_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'))
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let mut cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim_start_matches('.').to_owned();
    if cleaned.is_empty() {
        cleaned = "attachment".into();
    }
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_lowercase();
    if RESERVED_WINDOWS.contains(&stem.as_str()) {
        cleaned.insert(0, '_');
    }
    // Keep names within common file-system limits (in characters).
    if cleaned.chars().count() > 180 {
        let ext: String = cleaned.rsplit_once('.').map(|(_, e)| e).filter(|e| e.len() <= 10).map(|e| format!(".{e}")).unwrap_or_default();
        let head: String = cleaned.chars().take(180 - ext.chars().count()).collect();
        cleaned = format!("{head}{ext}");
    }
    cleaned
}

/// Whether opening the file would run code (including double extensions such as
/// `invoice.pdf.exe`).
pub fn is_executable(name: &str) -> bool {
    name.rsplit_once('.').map(|(_, ext)| EXECUTABLE.contains(&ext.trim().to_ascii_lowercase().as_str())).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_dangerous_names() {
        assert_eq!(sanitise_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitise_file_name("C:\\Windows\\x.dll"), "C__Windows_x.dll");
        assert_eq!(sanitise_file_name("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(sanitise_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitise_file_name("report.pdf. . "), "report.pdf");
        assert_eq!(sanitise_file_name("..."), "attachment");
        assert_eq!(sanitise_file_name("見積書.pdf"), "見積書.pdf");
        let long = format!("{}.pdf", "あ".repeat(300));
        let cut = sanitise_file_name(&long);
        assert!(cut.chars().count() <= 180 && cut.ends_with(".pdf"));
    }

    #[test]
    fn flags_executables() {
        assert!(is_executable("setup.EXE"));
        assert!(is_executable("invoice.pdf.exe"));
        assert!(is_executable("a.lnk"));
        assert!(!is_executable("photo.jpg"));
        assert!(!is_executable("README"));
    }
}
