//! The registry layout, as data so it can be checked off Windows.

pub const CLSID: &str = "{fbcd8210-9f9c-4b07-900a-ad12500a4363}";
pub const CLSID_U128: u128 = 0xfbcd8210_9f9c_4b07_900a_ad12500a4363;
pub const DESCRIPTION: &str = "Provenance property page";

const CLASSES: &str = r"Software\Classes";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved";

#[derive(Debug, PartialEq, Eq)]
pub struct Value {
    /// Key under HKEY_LOCAL_MACHINE.
    pub key: String,
    /// `None` is the key's default value.
    pub name: Option<&'static str>,
    pub data: String,
}

pub fn handler_key() -> String {
    format!(r"{CLASSES}\exefile\shellex\PropertySheetHandlers\SigstoreShell")
}

pub fn clsid_key() -> String {
    format!(r"{CLASSES}\CLSID\{CLSID}")
}

pub fn values(dll_path: &str) -> Vec<Value> {
    let v = |key: String, name, data: &str| Value {
        key,
        name,
        data: data.to_string(),
    };
    vec![
        v(clsid_key(), None, DESCRIPTION),
        v(format!(r"{}\InprocServer32", clsid_key()), None, dll_path),
        v(
            format!(r"{}\InprocServer32", clsid_key()),
            Some("ThreadingModel"),
            "Apartment",
        ),
        v(handler_key(), None, CLSID),
        v(APPROVED.to_string(), Some(CLSID), DESCRIPTION),
    ]
}

/// Whole keys to delete on unregister, plus the one value in the shared Approved key.
pub fn keys_to_delete() -> Vec<String> {
    vec![handler_key(), clsid_key()]
}

pub fn approved_key() -> &'static str {
    APPROVED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_clsid_handler_and_approved_entry() {
        let vals = values(r"C:\Program Files\sigstore-shell\sigstore_shell_ext.dll");
        let find = |key_suffix: &str, name: Option<&str>| {
            vals.iter()
                .find(|v| v.key.ends_with(key_suffix) && v.name == name)
                .map(|v| v.data.as_str())
        };
        assert_eq!(
            find(r"PropertySheetHandlers\SigstoreShell", None),
            Some(CLSID)
        );
        assert_eq!(
            find(r"InprocServer32", None),
            Some(r"C:\Program Files\sigstore-shell\sigstore_shell_ext.dll")
        );
        assert_eq!(
            find("InprocServer32", Some("ThreadingModel")),
            Some("Apartment")
        );
        assert_eq!(find("Approved", Some(CLSID)), Some(DESCRIPTION));
    }

    #[test]
    fn clsid_constants_agree() {
        let n = CLSID_U128;
        let text = format!(
            "{{{:08x}-{:04x}-{:04x}-{:04x}-{:012x}}}",
            n >> 96,
            (n >> 80) & 0xffff,
            (n >> 64) & 0xffff,
            (n >> 48) & 0xffff,
            n & 0xffff_ffff_ffff
        );
        assert_eq!(text, CLSID);
    }

    #[test]
    fn handler_lives_under_exefile_in_hklm_classes() {
        assert!(handler_key().starts_with(r"Software\Classes\exefile\shellex\"));
        assert!(CLSID.starts_with('{') && CLSID.ends_with('}'));
    }

    #[test]
    fn unregister_removes_what_register_created_except_the_shared_key() {
        let del = keys_to_delete();
        for v in values("x") {
            assert!(
                del.iter().any(|d| v.key.starts_with(d.as_str())) || v.key == approved_key(),
                "{} would be left behind",
                v.key
            );
        }
    }

    #[test]
    fn installer_script_writes_the_same_keys() {
        let nsi = include_str!("../../../installer/sigstore-shell.nsi").replace('\\', "/");
        let flat = |k: &str| k.replace('\\', "/");
        assert!(nsi.contains(CLSID));
        assert!(nsi.contains(DESCRIPTION));
        assert!(nsi.contains("ThreadingModel"));
        let handler = flat(&handler_key());
        assert!(nsi.contains(&handler), "{handler}");
        assert!(nsi.contains(&flat(approved_key())));
    }
}
