//! Jansson 2.14 calls. Compact and pretty dumps go through `json_dumps` so
//! the text matches the C file. The crate does not link libjansson; libobs
//! already does, and `cargo test` gets the symbols from the vendored copy
//! in `obs-c-oracle`.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};

use crate::data::{Array, Data, Rendered};

const JSON_OBJECT: i32 = 0;
const JSON_ARRAY: i32 = 1;
const JSON_STRING: i32 = 2;
const JSON_INTEGER: i32 = 3;
const JSON_REAL: i32 = 4;
const JSON_TRUE: i32 = 5;
const JSON_FALSE: i32 = 6;
const JSON_NULL: i32 = 7;

const JSON_REJECT_DUPLICATES: usize = 0x1;
const JSON_COMPACT: usize = 0x20;
const JSON_PRESERVE_ORDER: usize = 0x100;
const JSON_INDENT_4: usize = 4;

const LOG_ERROR: c_int = 100;

const TEXT_LEN: usize = 160;
const SOURCE_LEN: usize = 80;

#[repr(C)]
struct JsonError {
    line: c_int,
    column: c_int,
    position: c_int,
    source: [c_char; SOURCE_LEN],
    text: [c_char; TEXT_LEN],
}

unsafe extern "C" {
    fn blog(level: c_int, fmt: *const c_char, ...);
    fn json_loads(input: *const c_char, flags: usize, error: *mut JsonError) -> *mut c_void;
    fn json_dumps(json: *mut c_void, flags: usize) -> *mut c_char;
    fn json_object() -> *mut c_void;
    fn json_array() -> *mut c_void;
    fn json_string(value: *const c_char) -> *mut c_void;
    fn json_integer(value: i64) -> *mut c_void;
    fn json_real(value: f64) -> *mut c_void;
    fn json_true() -> *mut c_void;
    fn json_false() -> *mut c_void;
    fn json_null() -> *mut c_void;
    fn json_object_set_new(object: *mut c_void, key: *const c_char, value: *mut c_void) -> c_int;
    fn json_array_append_new(array: *mut c_void, value: *mut c_void) -> c_int;
    fn json_object_iter(object: *mut c_void) -> *mut c_void;
    fn json_object_iter_next(object: *mut c_void, iter: *mut c_void) -> *mut c_void;
    fn json_object_iter_key(iter: *mut c_void) -> *const c_char;
    fn json_object_iter_value(iter: *mut c_void) -> *mut c_void;
    fn json_array_size(array: *mut c_void) -> usize;
    fn json_array_get(array: *mut c_void, index: usize) -> *mut c_void;
    fn json_string_value(string: *mut c_void) -> *const c_char;
    fn json_integer_value(integer: *mut c_void) -> i64;
    fn json_real_value(real: *mut c_void) -> f64;
    fn obs_data_json_decref(json: *mut c_void);
}

/// Owned JSON tree. Object keys stay in insertion order.
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    Object(Vec<(String, J)>),
    Array(Vec<J>),
}

fn json_type(value: *mut c_void) -> i32 {
    unsafe { *(value as *const i32) }
}

fn c_string(text: &str) -> Option<CString> {
    CString::new(text).ok()
}

fn blog_parse(line: c_int, text: *const c_char) {
    let fmt = c"obs-data.c: [obs_data_create_from_json] Failed reading json string (%d): %s";
    unsafe { blog(LOG_ERROR, fmt.as_ptr(), line, text) }
}

/// `json_loads` with `JSON_REJECT_DUPLICATES` and without `JSON_DECODE_ANY`.
/// A failure is logged the way `obs_data_create_from_json` logs it.
pub fn loads(text: &str) -> Option<J> {
    let Ok(input) = CString::new(text) else {
        let msg = c"invalid string";
        blog_parse(1, msg.as_ptr());
        return None;
    };
    let mut error = JsonError {
        line: 0,
        column: 0,
        position: 0,
        source: [0; SOURCE_LEN],
        text: [0; TEXT_LEN],
    };
    let root = unsafe { json_loads(input.as_ptr(), JSON_REJECT_DUPLICATES, &mut error) };
    if root.is_null() {
        blog_parse(error.line, error.text.as_ptr());
        return None;
    }
    let value = convert(root);
    unsafe { obs_data_json_decref(root) };
    Some(value)
}

fn convert(value: *mut c_void) -> J {
    if value.is_null() {
        return J::Null;
    }
    match json_type(value) {
        JSON_OBJECT => {
            let mut pairs = Vec::new();
            let mut iter = unsafe { json_object_iter(value) };
            while !iter.is_null() {
                let key_ptr = unsafe { json_object_iter_key(iter) };
                let key = unsafe { CStr::from_ptr(key_ptr) }
                    .to_string_lossy()
                    .into_owned();
                let child = unsafe { json_object_iter_value(iter) };
                pairs.push((key, convert(child)));
                iter = unsafe { json_object_iter_next(value, iter) };
            }
            J::Object(pairs)
        }
        JSON_ARRAY => {
            let n = unsafe { json_array_size(value) };
            let mut items = Vec::with_capacity(n);
            for i in 0..n {
                items.push(convert(unsafe { json_array_get(value, i) }));
            }
            J::Array(items)
        }
        JSON_STRING => {
            let ptr = unsafe { json_string_value(value) };
            let text = if ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            };
            J::String(text)
        }
        JSON_INTEGER => J::Int(unsafe { json_integer_value(value) }),
        JSON_REAL => J::Real(unsafe { json_real_value(value) }),
        JSON_TRUE => J::Bool(true),
        JSON_FALSE => J::Bool(false),
        JSON_NULL => J::Null,
        _ => J::Null,
    }
}

/// Parses `text` into a [`Data`] without an intermediate tree.
///
/// The tree the parser builds is walked once, in the order jansson reports it,
/// so the object keeps the JSON order. The rules follow
/// `obs_data_add_json_item` in `libobs/obs-data.c.h`: an array keeps only its
/// object elements, a null stores an object item with no value
/// (`set_obj(key, None)`, like `obs_data_add_json_null`), and any other JSON
/// type is ignored.
pub fn load_data(text: &str) -> Option<Data> {
    load_data_bytes(text.as_bytes())
}

/// Lets Jansson validate the original JSON bytes, including their UTF-8.
pub(crate) fn load_data_bytes(text: &[u8]) -> Option<Data> {
    let Ok(input) = CString::new(text) else {
        let msg = c"invalid string";
        blog_parse(1, msg.as_ptr());
        return None;
    };
    let mut error = JsonError {
        line: 0,
        column: 0,
        position: 0,
        source: [0; SOURCE_LEN],
        text: [0; TEXT_LEN],
    };
    let root = unsafe { json_loads(input.as_ptr(), JSON_REJECT_DUPLICATES, &mut error) };
    if root.is_null() {
        blog_parse(error.line, error.text.as_ptr());
        return None;
    }
    let data = Data::new();
    match json_type(root) {
        JSON_OBJECT => fill_object(&data, root),
        // An array root has nothing for object iteration to walk.
        JSON_ARRAY => {}
        _ => {
            unsafe { obs_data_json_decref(root) };
            return None;
        }
    }
    unsafe { obs_data_json_decref(root) };
    Some(data)
}

/// Fills `data` with the members of the C json object `object`.
fn fill_object(data: &Data, object: *mut c_void) {
    let mut iter = unsafe { json_object_iter(object) };
    while !iter.is_null() {
        // SAFETY: jansson owns the key and the value, and keeps them alive
        // until the tree is freed.
        let key = unsafe { CStr::from_ptr(json_object_iter_key(iter)) }.to_string_lossy();
        let value = unsafe { json_object_iter_value(iter) };
        fill_value(data, &key, value);
        iter = unsafe { json_object_iter_next(object, iter) };
    }
}

/// Fills `data` with the C json value `value` under `key`.
fn fill_value(data: &Data, key: &str, value: *mut c_void) {
    match json_type(value) {
        JSON_OBJECT => {
            let child = Data::new();
            fill_object(&child, value);
            data.set_obj(key, Some(&child));
        }
        JSON_ARRAY => {
            let array = Array::new();
            let count = unsafe { json_array_size(value) };
            for i in 0..count {
                // SAFETY: the array owns its elements.
                let item = unsafe { json_array_get(value, i) };
                if json_type(item) != JSON_OBJECT {
                    continue;
                }
                let child = Data::new();
                fill_object(&child, item);
                array.push(Some(&child));
            }
            data.set_array(key, Some(&array));
        }
        JSON_STRING => {
            // SAFETY: the string belongs to the tree.
            let ptr = unsafe { json_string_value(value) };
            let text = if ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            };
            data.set_string(key, Some(&text));
        }
        JSON_INTEGER => {
            // SAFETY: the value is an integer.
            data.set_int(key, unsafe { json_integer_value(value) });
        }
        JSON_REAL => {
            // SAFETY: the value is a real.
            data.set_double(key, unsafe { json_real_value(value) });
        }
        JSON_TRUE => data.set_bool(key, true),
        JSON_FALSE => data.set_bool(key, false),
        JSON_NULL => data.set_obj(key, None),
        _ => {}
    }
}

/// Serializes `data` into the text jansson writes.
///
/// The C json tree is built straight from the object, so the document does not
/// pass through an intermediate Rust tree on the way out.
pub fn dump_data(data: &Data, pretty: bool, with_defaults: bool) -> Option<CString> {
    let root = build_object(data, with_defaults)?;
    let flags = JSON_PRESERVE_ORDER | if pretty { JSON_INDENT_4 } else { JSON_COMPACT };
    // SAFETY: the tree was built here and is not used after the dump.
    let ptr = unsafe { json_dumps(root, flags) };
    unsafe { obs_data_json_decref(root) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: jansson wrote the buffer with malloc; the copy owns its memory.
    let text = unsafe { CStr::from_ptr(ptr) }.to_owned();
    unsafe { libc::free(ptr as *mut c_void) };
    Some(text)
}

/// Builds the C json object for `data`.
fn build_object(data: &Data, with_defaults: bool) -> Option<*mut c_void> {
    // SAFETY: the object is freed on every path that leaves this function.
    let object = unsafe { json_object() };
    if object.is_null() {
        return None;
    }
    for (name, rendered) in data.rendered_items(with_defaults) {
        // C omits a real that Jansson cannot represent, keeping other settings.
        if matches!(&rendered, Rendered::Real(value) if !value.is_finite()) {
            continue;
        }
        let Some(child) = build_value(&rendered, with_defaults) else {
            unsafe { obs_data_json_decref(object) };
            return None;
        };
        let Ok(key) = CString::new(name) else {
            unsafe { obs_data_json_decref(child) };
            unsafe { obs_data_json_decref(object) };
            return None;
        };
        // SAFETY: the object and the child are both live json values.
        if unsafe { json_object_set_new(object, key.as_ptr(), child) } != 0 {
            unsafe { obs_data_json_decref(child) };
            unsafe { obs_data_json_decref(object) };
            return None;
        }
    }
    Some(object)
}

/// Builds the C json value for one rendered item.
fn build_value(rendered: &Rendered, with_defaults: bool) -> Option<*mut c_void> {
    match rendered {
        Rendered::String(text) => {
            let text = CString::new(text.as_str()).ok()?;
            // SAFETY: the string is copied into the tree.
            Some(unsafe { json_string(text.as_ptr()) })
        }
        Rendered::Int(value) => {
            // SAFETY: the value is an integer.
            Some(unsafe { json_integer(*value) })
        }
        Rendered::Real(value) => {
            // SAFETY: the value is a real.
            Some(unsafe { json_real(*value) })
        }
        Rendered::Bool(value) => {
            // SAFETY: the value is a boolean.
            Some(unsafe { if *value { json_true() } else { json_false() } })
        }
        Rendered::Object(obj) => match obj {
            Some(data) => build_object(data, with_defaults),
            None => {
                // SAFETY: the value is a null.
                Some(unsafe { json_null() })
            }
        },
        Rendered::Array(array) => {
            // SAFETY: the array is freed on every path that leaves here.
            let json = unsafe { json_array() };
            if json.is_null() {
                return None;
            }
            // An absent array serializes as `[]`, like `into_j` did.
            if let Some(array) = array {
                for element in array.elements() {
                    let Some(child) = build_object(&element, with_defaults) else {
                        unsafe { obs_data_json_decref(json) };
                        return None;
                    };
                    // SAFETY: the array and the child are both live json values.
                    if unsafe { json_array_append_new(json, child) } != 0 {
                        unsafe { obs_data_json_decref(child) };
                        unsafe { obs_data_json_decref(json) };
                        return None;
                    }
                }
            }
            Some(json)
        }
    }
}

/// `json_dumps` with `JSON_PRESERVE_ORDER` and either compact or indent 4.
pub fn dumps(value: &J, pretty: bool) -> Option<String> {
    let root = build(value)?;
    let flags = JSON_PRESERVE_ORDER | if pretty { JSON_INDENT_4 } else { JSON_COMPACT };
    let ptr = unsafe { json_dumps(root, flags) };
    unsafe { obs_data_json_decref(root) };
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(ptr as *mut c_void) };
    Some(text)
}

fn build(value: &J) -> Option<*mut c_void> {
    let ptr = match value {
        J::Null => unsafe { json_null() },
        J::Bool(true) => unsafe { json_true() },
        J::Bool(false) => unsafe { json_false() },
        J::Int(v) => unsafe { json_integer(*v) },
        J::Real(v) => unsafe { json_real(*v) },
        J::String(s) => {
            let c = c_string(s)?;
            unsafe { json_string(c.as_ptr()) }
        }
        J::Array(items) => {
            let array = unsafe { json_array() };
            if array.is_null() {
                return None;
            }
            for item in items {
                let Some(child) = build(item) else {
                    unsafe { obs_data_json_decref(array) };
                    return None;
                };
                if unsafe { json_array_append_new(array, child) } != 0 {
                    unsafe { obs_data_json_decref(child) };
                    unsafe { obs_data_json_decref(array) };
                    return None;
                }
            }
            array
        }
        J::Object(pairs) => {
            let object = unsafe { json_object() };
            if object.is_null() {
                return None;
            }
            for (key, child) in pairs {
                let key_c = match c_string(key) {
                    Some(k) => k,
                    None => {
                        unsafe { obs_data_json_decref(object) };
                        return None;
                    }
                };
                let child = match build(child) {
                    Some(c) => c,
                    None => {
                        unsafe { obs_data_json_decref(object) };
                        return None;
                    }
                };
                if unsafe { json_object_set_new(object, key_c.as_ptr(), child) } != 0 {
                    unsafe { obs_data_json_decref(child) };
                    unsafe { obs_data_json_decref(object) };
                    return None;
                }
            }
            object
        }
    };
    if ptr.is_null() { None } else { Some(ptr) }
}
