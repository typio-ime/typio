//! String utilities and the canonical libtypio deallocator family.
//!
//! All `char *` returns documented as libtypio-owned are produced through
//! `CString::into_raw` and must be released via [`typio_free_string`]; the
//! list returns from `typio_registry_list_*` are released via
//! [`typio_free_string_array`]. Mixing these with libc `free()` is undefined
//! behaviour and on Windows will corrupt the heap (different CRTs).

use std::ffi::{CStr, CString, c_char, c_double, c_int};
use std::slice;

fn cstr_into_raw(bytes: &[u8]) -> *mut c_char {
    match CString::new(bytes) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Duplicate a C string (libc `strdup` equivalent using libtypio allocator).
///
/// Returns NULL when `str` is NULL. Caller must free with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_strdup(str: *const c_char) -> *mut c_char {
    if str.is_null() {
        return std::ptr::null_mut();
    }
    let bytes = unsafe { CStr::from_ptr(str) }.to_bytes();
    cstr_into_raw(bytes)
}

/// Duplicate at most `n` bytes of a C string.
///
/// Returns NULL when `str` is NULL. Caller must free with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_strndup(str: *const c_char, n: usize) -> *mut c_char {
    if str.is_null() {
        return std::ptr::null_mut();
    }
    let bytes = unsafe { slice::from_raw_parts(str as *const u8, n) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(n);
    cstr_into_raw(&bytes[..len])
}

/// Concatenate two C strings.
///
/// Returns NULL if both are NULL. Caller must free with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_strjoin(a: *const c_char, b: *const c_char) -> *mut c_char {
    if a.is_null() && b.is_null() {
        return std::ptr::null_mut();
    }
    if a.is_null() {
        return typio_strdup(b);
    }
    if b.is_null() {
        return typio_strdup(a);
    }
    let mut result: Vec<u8> = Vec::new();
    unsafe {
        result.extend_from_slice(CStr::from_ptr(a).to_bytes());
        result.extend_from_slice(CStr::from_ptr(b).to_bytes());
    }
    cstr_into_raw(&result)
}

/// Concatenate three C strings.
///
/// Returns NULL if the first two are NULL. Caller must free with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_strjoin3(
    a: *const c_char,
    b: *const c_char,
    c: *const c_char,
) -> *mut c_char {
    let ab = typio_strjoin(a, b);
    if ab.is_null() {
        return std::ptr::null_mut();
    }
    let result = typio_strjoin(ab, c);
    typio_free_string(ab);
    result
}

/// Join two path components with a `/` if needed.
///
/// Returns NULL if either argument is NULL. Caller must free with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_path_join(base: *const c_char, suffix: *const c_char) -> *mut c_char {
    if base.is_null() || suffix.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let base_str = CStr::from_ptr(base).to_str().unwrap_or("");
        let suffix_str = CStr::from_ptr(suffix).to_str().unwrap_or("");
        let need_slash = !base_str.is_empty() && !base_str.ends_with('/');
        let joined = if need_slash {
            format!("{}/{}", base_str, suffix_str)
        } else {
            format!("{}{}", base_str, suffix_str)
        };
        cstr_into_raw(joined.as_bytes())
    }
}

/// Return true if `str` starts with `prefix`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_starts_with(str: *const c_char, prefix: *const c_char) -> bool {
    if str.is_null() || prefix.is_null() {
        return false;
    }
    unsafe {
        let s = CStr::from_ptr(str).to_bytes();
        let p = CStr::from_ptr(prefix).to_bytes();
        s.starts_with(p)
    }
}

/// Return true if `str` ends with `suffix`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_ends_with(str: *const c_char, suffix: *const c_char) -> bool {
    if str.is_null() || suffix.is_null() {
        return false;
    }
    unsafe {
        let s = CStr::from_ptr(str).to_bytes();
        let suf = CStr::from_ptr(suffix).to_bytes();
        s.ends_with(suf)
    }
}

/// Return true if two C strings are byte-for-byte equal.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_equals(a: *const c_char, b: *const c_char) -> bool {
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe { CStr::from_ptr(a).to_bytes() == CStr::from_ptr(b).to_bytes() }
}

/// Case-insensitive ASCII comparison of two C strings.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_equals_nocase(a: *const c_char, b: *const c_char) -> bool {
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe {
        let a_str = CStr::from_ptr(a).to_str().unwrap_or("");
        let b_str = CStr::from_ptr(b).to_str().unwrap_or("");
        a_str.eq_ignore_ascii_case(b_str)
    }
}

/// Find the first occurrence of `needle` in `haystack`.
///
/// Returns a pointer into `haystack` (not a new allocation), or NULL if not found.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_find(haystack: *const c_char, needle: *const c_char) -> *const c_char {
    if haystack.is_null() || needle.is_null() {
        return std::ptr::null();
    }
    unsafe {
        let h = CStr::from_ptr(haystack).to_str().unwrap_or("");
        let n = CStr::from_ptr(needle).to_str().unwrap_or("");
        match h.find(n) {
            Some(pos) => haystack.add(pos),
            None => std::ptr::null(),
        }
    }
}

/// Parse a C string as a signed 32-bit integer.
///
/// Returns `default_val` if the string is NULL or not a valid integer.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_to_int(str: *const c_char, default_val: c_int) -> c_int {
    if str.is_null() {
        return default_val;
    }
    unsafe {
        let s = CStr::from_ptr(str).to_str().unwrap_or("");
        s.parse::<c_int>().unwrap_or(default_val)
    }
}

/// Parse a C string as a 64-bit floating point number.
///
/// Returns `default_val` if the string is NULL or not a valid number.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_to_double(str: *const c_char, default_val: c_double) -> c_double {
    if str.is_null() {
        return default_val;
    }
    unsafe {
        let s = CStr::from_ptr(str).to_str().unwrap_or("");
        s.parse::<c_double>().unwrap_or(default_val)
    }
}

/// Parse a C string as a boolean.
///
/// Recognizes "true", "yes", "1", "on" and "false", "no", "0", "off".
/// Returns `default_val` if the string is NULL or not recognized.
#[unsafe(no_mangle)]
pub extern "C" fn typio_str_to_bool(str: *const c_char, default_val: bool) -> bool {
    if str.is_null() {
        return default_val;
    }
    unsafe {
        let s = CStr::from_ptr(str).to_str().unwrap_or("").to_lowercase();
        match s.as_str() {
            "true" | "yes" | "1" | "on" => true,
            "false" | "no" | "0" | "off" => false,
            _ => default_val,
        }
    }
}

/* UTF-8 utilities */

/// Return the number of Unicode scalar values in a UTF-8 string.
#[unsafe(no_mangle)]
pub extern "C" fn typio_utf8_strlen(str: *const c_char) -> usize {
    if str.is_null() {
        return 0;
    }
    unsafe {
        let bytes = CStr::from_ptr(str).to_bytes();
        bytes.iter().filter(|&&b| (b & 0xC0) != 0x80).count()
    }
}

/// Advance to the next UTF-8 code point.
///
/// Returns `str` if it is NULL or points to a NUL byte.
#[unsafe(no_mangle)]
pub extern "C" fn typio_utf8_next(str: *const c_char) -> *const c_char {
    if str.is_null() || unsafe { *str } == 0 {
        return str;
    }
    unsafe {
        let mut p = str.add(1);
        while *p != 0 && (*p as u8 & 0xC0) == 0x80 {
            p = p.add(1);
        }
        p
    }
}

/// Step back to the previous UTF-8 code point.
///
/// Returns `start` if `str` is at or before `start`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_utf8_prev(str: *const c_char, start: *const c_char) -> *const c_char {
    if str.is_null() || start.is_null() || str <= start {
        return start;
    }
    unsafe {
        let mut p = str.sub(1);
        while p > start && (*p as u8 & 0xC0) == 0x80 {
            p = p.sub(1);
        }
        p
    }
}

/// Decode the first UTF-8 code point at `str`.
///
/// Returns 0 if `str` is NULL or empty. Returns U+FFFD for invalid sequences.
#[unsafe(no_mangle)]
pub extern "C" fn typio_utf8_get_char(str: *const c_char) -> u32 {
    if str.is_null() || unsafe { *str } == 0 {
        return 0;
    }
    unsafe {
        let c = *str as u8;
        if c < 0x80 {
            return c as u32;
        }
        let (mut result, remaining) = if (c & 0xE0) == 0xC0 {
            (c as u32 & 0x1F, 1)
        } else if (c & 0xF0) == 0xE0 {
            (c as u32 & 0x0F, 2)
        } else if (c & 0xF8) == 0xF0 {
            (c as u32 & 0x07, 3)
        } else {
            return 0xFFFD;
        };
        let mut p = str.add(1);
        for _ in 0..remaining {
            if *p == 0 || (*p as u8 & 0xC0) != 0x80 {
                return 0xFFFD;
            }
            result = (result << 6) | ((*p as u8) & 0x3F) as u32;
            p = p.add(1);
        }
        result
    }
}

/// Encode a Unicode code point into UTF-8 at `buf`.
///
/// Returns the number of bytes written (1–4), or 0 if `buf` is NULL or the
/// code point is out of range.
#[unsafe(no_mangle)]
pub extern "C" fn typio_utf8_encode(codepoint: u32, buf: *mut c_char) -> usize {
    if buf.is_null() {
        return 0;
    }
    unsafe {
        if codepoint < 0x80 {
            *buf = codepoint as c_char;
            return 1;
        }
        if codepoint < 0x800 {
            *buf = (0xC0 | (codepoint >> 6)) as c_char;
            *buf.add(1) = (0x80 | (codepoint & 0x3F)) as c_char;
            return 2;
        }
        if codepoint < 0x10000 {
            *buf = (0xE0 | (codepoint >> 12)) as c_char;
            *buf.add(1) = (0x80 | ((codepoint >> 6) & 0x3F)) as c_char;
            *buf.add(2) = (0x80 | (codepoint & 0x3F)) as c_char;
            return 3;
        }
        if codepoint < 0x110000 {
            *buf = (0xF0 | (codepoint >> 18)) as c_char;
            *buf.add(1) = (0x80 | ((codepoint >> 12) & 0x3F)) as c_char;
            *buf.add(2) = (0x80 | ((codepoint >> 6) & 0x3F)) as c_char;
            *buf.add(3) = (0x80 | (codepoint & 0x3F)) as c_char;
            return 4;
        }
        0
    }
}

/// Free a string previously returned by any libtypio function.
///
/// No-op when `str` is NULL. Do not pass strings allocated by the C caller —
/// use the matching allocator for those.
#[unsafe(no_mangle)]
pub extern "C" fn typio_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Free a name list previously returned by any `typio_registry_list_*` call.
#[unsafe(no_mangle)]
pub extern "C" fn typio_free_string_array(list: *mut *mut c_char, count: usize) {
    if list.is_null() {
        return;
    }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(list, count);
        for entry in slice {
            if !entry.is_null() {
                drop(CString::from_raw(*entry));
            }
        }
        let fat = std::ptr::slice_from_raw_parts_mut(list, count);
        drop(Box::from_raw(fat));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::ptr;

    #[test]
    fn strdup_roundtrip() {
        let original = CString::new("hello world").unwrap();
        let dup = typio_strdup(original.as_ptr());
        assert!(!dup.is_null());
        let s = unsafe { CStr::from_ptr(dup) }.to_str().unwrap();
        assert_eq!(s, "hello world");
        typio_free_string(dup);
    }

    #[test]
    fn strdup_null_returns_null() {
        let dup = typio_strdup(ptr::null());
        assert!(dup.is_null());
    }
}
