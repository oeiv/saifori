//! GNOME Secret Service storage via libsecret. No API key is ever written to
//! application settings, SQLite, CSV or backup files.
//!
//! The small FFI boundary is intentionally isolated here and audited against
//! libsecret's public C API. Calls may block: invoke only on worker threads.
//!
//! The default keyring/portal controls encryption, unlocking and access.
use anyhow::{anyhow, bail, Result};
use std::{
    ffi::{CStr, CString},
    os::raw::{c_char, c_int, c_void},
    ptr,
};

const SCHEMA_NAME: &[u8] = b"io.github.oeiv.Saifori.Credentials\0";
// Compatibility only. The previous app's credential is moved once, after a
// successful write under the new schema; no old branding is displayed.
const LEGACY_SCHEMA_NAME: &[u8] = b"io.github.sarf.ExpenseTracker.Credentials\0";
const ATTRIBUTE_NAME: &[u8] = b"provider\0";

// These GLib-compatible C interfaces are stable in libsecret 1.x.
#[link(name = "secret-1")]
extern "C" {
    fn secret_schema_new(name: *const c_char, flags: c_int, ...) -> *mut c_void;
    fn secret_schema_unref(schema: *mut c_void);
    fn secret_password_store_sync(
        schema: *const c_void,
        collection: *const c_char,
        label: *const c_char,
        password: *const c_char,
        cancellable: *mut c_void,
        error: *mut *mut gtk::glib::ffi::GError,
        ...
    ) -> c_int;
    fn secret_password_lookup_sync(
        schema: *const c_void,
        cancellable: *mut c_void,
        error: *mut *mut gtk::glib::ffi::GError,
        ...
    ) -> *mut c_char;
    fn secret_password_clear_sync(
        schema: *const c_void,
        cancellable: *mut c_void,
        error: *mut *mut gtk::glib::ffi::GError,
        ...
    ) -> c_int;
    fn secret_password_free(password: *mut c_char);
}

fn as_c(bytes: &'static [u8]) -> *const c_char {
    bytes.as_ptr().cast()
}

fn valid_provider(provider: &str) -> bool {
    matches!(provider, "deepseek" | "claude" | "openai")
}

fn provider_value(provider: &str) -> Result<CString> {
    let normalized = provider.trim().to_lowercase();
    if !valid_provider(&normalized) {
        bail!("Unsupported AI provider");
    }
    CString::new(normalized).map_err(|_| anyhow!("Unsupported AI provider"))
}

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "DeepSeek",
        "claude" => "Claude",
        "openai" => "GPT / OpenAI",
        _ => "AI",
    }
}

fn item_label(provider: &str) -> Result<CString> {
    CString::new(format!("Saifori {} API key", provider_label(provider)))
        .map_err(|_| anyhow!("Invalid key label"))
}

struct Schema(*mut c_void);
impl Schema {
    fn new(schema_name: &'static [u8]) -> Result<Self> {
        // SAFETY: schema name and attribute name are NUL-terminated static
        // strings; attribute type 0 is SECRET_SCHEMA_ATTRIBUTE_STRING.
        // libsecret copies them and returns an owned schema pointer.
        let ptr = unsafe {
            secret_schema_new(
                as_c(schema_name),
                0 as c_int,
                as_c(ATTRIBUTE_NAME),
                0 as c_int,
                ptr::null::<c_char>(),
            )
        };
        if ptr.is_null() {
            bail!("Could not initialize GNOME Secret Service schema");
        }
        Ok(Self(ptr))
    }
}
impl Drop for Schema {
    fn drop(&mut self) {
        // SAFETY: ptr is non-NULL and owned, returned by secret_schema_new.
        unsafe { secret_schema_unref(self.0) };
    }
}

fn check_error(err: *mut gtk::glib::ffi::GError) -> Result<()> {
    if err.is_null() {
        return Ok(());
    }
    // Do not expose the keyring's error messages to the UI/logs; error text may
    // include user-specific item metadata on some backends.
    // SAFETY: err is allocated by GLib; g_error_free takes ownership.
    unsafe {
        gtk::glib::ffi::g_error_free(err);
    }
    Err(anyhow!(
        "Secure storage is unavailable or locked. Unlock GNOME Keyring and retry."
    ))
}

fn lookup_with_schema(schema_name: &'static [u8], provider: &CString) -> Result<Option<String>> {
    let schema = Schema::new(schema_name)?;
    let mut err = ptr::null_mut();
    // SAFETY: the schema and C string are alive, and varargs terminate with NULL.
    let raw = unsafe {
        secret_password_lookup_sync(
            schema.0,
            ptr::null_mut(),
            &mut err,
            as_c(ATTRIBUTE_NAME),
            provider.as_ptr(),
            ptr::null::<c_char>(),
        )
    };
    if let Err(failure) = check_error(err) {
        if !raw.is_null() {
            unsafe { secret_password_free(raw) };
        }
        return Err(failure);
    }
    if raw.is_null() {
        return Ok(None);
    }
    let result = unsafe { CStr::from_ptr(raw).to_str().map(str::to_owned) };
    unsafe { secret_password_free(raw) };
    Ok(Some(
        result.map_err(|_| anyhow!("Stored key has invalid encoding"))?,
    ))
}

/// If the prior credential exists, copy it into the new schema before clearing
/// the obsolete entry. This avoids exposing or logging the secret and keeps
/// each provider separate. A failed copy leaves the old credential intact.
pub fn lookup(provider: &str) -> Result<Option<String>> {
    let normalized = provider.trim().to_lowercase();
    let provider_c = provider_value(&normalized)?;
    if let Some(key) = lookup_with_schema(SCHEMA_NAME, &provider_c)? {
        return Ok(Some(key));
    }
    let Some(key) = lookup_with_schema(LEGACY_SCHEMA_NAME, &provider_c)? else {
        return Ok(None);
    };
    store(&normalized, &key)?;
    // If this fails the new entry is still safely stored; fail explicitly so
    // the user can retry rather than silently leaving a resurrectable legacy key.
    clear_with_schema(LEGACY_SCHEMA_NAME, &provider_c)?;
    Ok(Some(key))
}

pub fn store(provider: &str, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 1000 || key.chars().any(char::is_whitespace) {
        bail!("Invalid API key");
    }
    let provider = provider_value(provider)?;
    let label = item_label(provider.to_str().unwrap_or("deepseek"))?;
    let candidate = CString::new(key).map_err(|_| anyhow!("Invalid API key"))?;
    let schema = Schema::new(SCHEMA_NAME)?;
    let mut err = ptr::null_mut();
    // SAFETY: pointers remain valid for the synchronous operation. A NULL
    // collection selects the persistent DEFAULT collection, NOT SESSION.
    let saved = unsafe {
        secret_password_store_sync(
            schema.0,
            ptr::null(),
            label.as_ptr(),
            candidate.as_ptr(),
            ptr::null_mut(),
            &mut err,
            as_c(ATTRIBUTE_NAME),
            provider.as_ptr(),
            ptr::null::<c_char>(),
        )
    };
    check_error(err)?;
    if saved == 0 {
        bail!("GNOME Keyring did not confirm saving the key");
    }
    Ok(())
}

fn clear_with_schema(schema_name: &'static [u8], provider: &CString) -> Result<()> {
    let schema = Schema::new(schema_name)?;
    let mut err = ptr::null_mut();
    // SAFETY: all pointers remain valid for the synchronous call and the
    // varargs use a NUL-terminated attribute/value pair and a NULL sentinel.
    unsafe {
        secret_password_clear_sync(
            schema.0,
            ptr::null_mut(),
            &mut err,
            as_c(ATTRIBUTE_NAME),
            provider.as_ptr(),
            ptr::null::<c_char>(),
        );
    }
    check_error(err)
}

pub fn remove(provider: &str) -> Result<()> {
    let provider = provider_value(provider)?;
    // Clear BOTH schemas: a removed key must never reappear from the migration
    // fallback after relaunch. Neither item is shown or serialized anywhere.
    clear_with_schema(SCHEMA_NAME, &provider)?;
    clear_with_schema(LEGACY_SCHEMA_NAME, &provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn item_scope_and_schema_are_stable() {
        assert_eq!(SCHEMA_NAME, b"io.github.oeiv.Saifori.Credentials\0");
        assert!(LEGACY_SCHEMA_NAME.ends_with(b"Credentials\0"));
        assert_eq!(ATTRIBUTE_NAME, b"provider\0");
        assert!(valid_provider("deepseek"));
        assert!(valid_provider("claude"));
        assert!(valid_provider("openai"));
        assert!(!valid_provider("other"));
    }
}
