//! Local settings persistence regressions, with unchanged C obs-data as oracle.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;

use obs_c_oracle as _;
use obs_data::{Data, ffi::obs_data as ffi};
use obs_util as _;

#[path = "../../../obs-util/tests/common/mod.rs"]
mod common;

unsafe extern "C" {
    fn oracle_obs_data_create_from_json(text: *const c_char) -> *mut c_void;
    fn oracle_obs_data_create_from_json_file(path: *const c_char) -> *mut c_void;
    fn oracle_obs_data_create_from_json_file_safe(
        path: *const c_char,
        backup: *const c_char,
    ) -> *mut c_void;
    fn oracle_obs_data_release(data: *mut c_void);
    fn oracle_obs_data_get_json(data: *mut c_void) -> *const c_char;
    fn oracle_obs_data_get_json_pretty(data: *mut c_void) -> *const c_char;
    fn oracle_obs_data_get_json_with_defaults(data: *mut c_void) -> *const c_char;
    fn oracle_obs_data_get_json_pretty_with_defaults(data: *mut c_void) -> *const c_char;
    fn oracle_obs_data_set_double(data: *mut c_void, name: *const c_char, value: f64);
    fn oracle_obs_data_set_default_double(data: *mut c_void, name: *const c_char, value: f64);
    fn oracle_obs_data_get_obj(data: *mut c_void, name: *const c_char) -> *mut c_void;
    fn oracle_obs_data_get_array(data: *mut c_void, name: *const c_char) -> *mut c_void;
    fn oracle_obs_data_array_item(array: *mut c_void, index: usize) -> *mut c_void;
    fn oracle_obs_data_array_release(array: *mut c_void);
    fn oracle_obs_data_save_json(data: *mut c_void, path: *const c_char) -> bool;
    fn oracle_obs_data_save_json_safe(
        data: *mut c_void,
        path: *const c_char,
        temp: *const c_char,
        backup: *const c_char,
    ) -> bool;
    fn oracle_obs_data_save_json_pretty_safe(
        data: *mut c_void,
        path: *const c_char,
        temp: *const c_char,
        backup: *const c_char,
    ) -> bool;
}

fn path_c(path: &Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}

type Dump = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type Release = unsafe extern "C" fn(*mut c_void);
type SaveSafe =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> bool;

fn snapshot(data: *mut c_void, dump: Dump) -> Option<String> {
    if data.is_null() {
        return None;
    }
    // SAFETY: the caller supplies a live object and its implementation's dump.
    let text = unsafe { dump(data) };
    if text.is_null() {
        return None;
    }
    // SAFETY: copy the cached C string before its object is released/dumped again.
    Some(unsafe { CStr::from_ptr(text) }.to_str().unwrap().to_owned())
}

fn consume(data: *mut c_void, dump: Dump, release: Release) -> Option<String> {
    let text = snapshot(data, dump);
    // SAFETY: this consumes the returned reference; release also accepts null.
    unsafe { release(data) };
    text
}

const FIXTURE: &str = r#"{"keep":7,"child":{"keep":8},"array":[{"keep":9},{}],"null":null}"#;
const INVALID: &[&[u8]] = &[
    b"{\"name\":\"\xff\"}",
    b"{\"\xc0\xaf\":1}",
    b"{\"child\":{\"name\":\"\xed\xa0\x80\"}}",
    b"{\"array\":[{\"name\":\"\xe2\x82\"}]}",
];
const VALID: &str = "{\"name\":\"café 日本語 😀 �\"}";

#[test]
fn core_nonfinite_keeps_settings_and_saves() {
    let dir = common::ScratchDir::new("obs-data-nonfinite-core");
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let data = Data::from_json(FIXTURE).unwrap();
        data.set_double("bad", value);
        data.get_obj("child").unwrap().set_double("bad", value);
        let array = data.get_array("array").unwrap();
        for index in 0..array.count() {
            array.item(index).unwrap().set_double("bad", value);
        }
        assert_eq!(data.json(), FIXTURE);
        assert_eq!(
            Data::from_json(&data.json_pretty()).unwrap().json(),
            FIXTURE
        );
        assert!(data.has_user_value("bad"));

        data.set_default_double("bad_default", value);
        assert_eq!(data.json_with_defaults(), FIXTURE);
        assert_eq!(
            Data::from_json(&data.json_pretty_with_defaults())
                .unwrap()
                .json(),
            FIXTURE
        );
        let path = dir.join("settings.json");
        assert!(data.save_json(&path));
        assert!(data.save_json_safe(&path, "tmp", "bak"));
        assert!(data.save_json_pretty_safe(&path, "tmp", "bak"));
        assert_eq!(Data::from_json_file(&path).unwrap().json(), FIXTURE);
    }
}

#[test]
fn core_invalid_utf8_rejects_files() {
    let dir = common::ScratchDir::new("obs-data-utf8-core-reject");
    let path = dir.join("settings.json");
    for bytes in INVALID {
        std::fs::write(&path, bytes).unwrap();
        assert!(Data::from_json_file(&path).is_none());
        assert!(Data::from_json_file_safe(&path, "bak").is_none());
    }
}

#[test]
fn core_invalid_utf8_recovers_backup() {
    let dir = common::ScratchDir::new("obs-data-utf8-core");
    let path = dir.join("settings.json");
    let backup = dir.join("settings.json.bak");
    for bytes in INVALID {
        std::fs::write(&path, bytes).unwrap();
        std::fs::write(&backup, VALID).unwrap();
        assert_eq!(
            Data::from_json_file_safe(&path, ".bak").unwrap().json(),
            VALID
        );
        assert_eq!(std::fs::read(&path).unwrap(), VALID.as_bytes());
        assert!(!backup.exists());
    }
    for prefix in [b"".as_slice(), b"\xef\xbb\xbf"] {
        let bytes = [prefix, VALID.as_bytes()].concat();
        std::fs::write(&path, &bytes).unwrap();
        std::fs::write(&backup, b"{\"backup\":true}").unwrap();
        assert_eq!(Data::from_json_file(&path).unwrap().json(), VALID);
        assert_eq!(
            Data::from_json_file_safe(&path, "bak").unwrap().json(),
            VALID
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(backup.exists());
    }
}

#[test]
fn nonfinite_dumps_and_saves_match_c() {
    if !common::isolated(concat!(
        module_path!(),
        "::nonfinite_dumps_and_saves_match_c"
    )) {
        return;
    }
    let dir = common::ScratchDir::new("obs-data-nonfinite-parity");
    let fixture = CString::new(FIXTURE).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.25] {
        // SAFETY: both implementations receive valid strings and their own handles.
        unsafe {
            let c = oracle_obs_data_create_from_json(fixture.as_ptr());
            let rust = ffi::obs_data_create_from_json(fixture.as_ptr());
            assert!(!c.is_null() && !rust.is_null());
            oracle_obs_data_set_double(c, c"bad".as_ptr(), value);
            ffi::obs_data_set_double(rust, c"bad".as_ptr(), value);
            oracle_obs_data_set_default_double(c, c"default".as_ptr(), value);
            ffi::obs_data_set_default_double(rust, c"default".as_ptr(), value);
            let c_child = oracle_obs_data_get_obj(c, c"child".as_ptr());
            let rust_child = ffi::obs_data_get_obj(rust, c"child".as_ptr());
            oracle_obs_data_set_double(c_child, c"bad".as_ptr(), value);
            ffi::obs_data_set_double(rust_child, c"bad".as_ptr(), value);
            oracle_obs_data_release(c_child);
            ffi::obs_data_release(rust_child);
            let c_array = oracle_obs_data_get_array(c, c"array".as_ptr());
            let rust_array = ffi::obs_data_get_array(rust, c"array".as_ptr());
            for index in 0..2 {
                let c_child = oracle_obs_data_array_item(c_array, index);
                let rust_child = ffi::obs_data_array_item(rust_array, index);
                oracle_obs_data_set_double(c_child, c"bad".as_ptr(), value);
                ffi::obs_data_set_double(rust_child, c"bad".as_ptr(), value);
                oracle_obs_data_release(c_child);
                ffi::obs_data_release(rust_child);
            }
            oracle_obs_data_array_release(c_array);
            ffi::obs_data_array_release(rust_array);

            for (c_dump, rust_dump) in [
                (
                    oracle_obs_data_get_json as Dump,
                    ffi::obs_data_get_json as Dump,
                ),
                (
                    oracle_obs_data_get_json_pretty,
                    ffi::obs_data_get_json_pretty,
                ),
                (
                    oracle_obs_data_get_json_with_defaults,
                    ffi::obs_data_get_json_with_defaults,
                ),
                (
                    oracle_obs_data_get_json_pretty_with_defaults,
                    ffi::obs_data_get_json_pretty_with_defaults,
                ),
            ] {
                let expected = snapshot(c, c_dump).unwrap();
                if !value.is_finite() {
                    assert_eq!(Data::from_json(&expected).unwrap().json(), FIXTURE);
                }
                assert_eq!(snapshot(rust, rust_dump), Some(expected));
            }
            let c_path = dir.join("c.json");
            let rust_path = dir.join("rust.json");
            let c_name = path_c(&c_path);
            let rust_name = path_c(&rust_path);
            assert!(oracle_obs_data_save_json(c, c_name.as_ptr()));
            assert!(ffi::obs_data_save_json(rust, rust_name.as_ptr()));
            assert_eq!(
                std::fs::read(&c_path).unwrap(),
                std::fs::read(&rust_path).unwrap()
            );
            for (c_save, rust_save) in [
                (
                    oracle_obs_data_save_json_safe as SaveSafe,
                    ffi::obs_data_save_json_safe as SaveSafe,
                ),
                (
                    oracle_obs_data_save_json_pretty_safe,
                    ffi::obs_data_save_json_pretty_safe,
                ),
            ] {
                assert!(c_save(c, c_name.as_ptr(), c"tmp".as_ptr(), c"bak".as_ptr()));
                assert!(rust_save(
                    rust,
                    rust_name.as_ptr(),
                    c"tmp".as_ptr(),
                    c"bak".as_ptr()
                ));
                assert_eq!(
                    std::fs::read(&c_path).unwrap(),
                    std::fs::read(&rust_path).unwrap()
                );
                assert_eq!(
                    std::fs::read(dir.join("c.json.bak")).unwrap(),
                    std::fs::read(dir.join("rust.json.bak")).unwrap()
                );
            }
            oracle_obs_data_release(c);
            ffi::obs_data_release(rust);
        }
    }
}

#[test]
fn direct_json_utf8_matches_c() {
    for bytes in INVALID.iter().copied().chain([VALID.as_bytes()]) {
        let input = CString::new(bytes).unwrap();
        // SAFETY: input is terminated and lives through both calls; each reference
        // is released by its own implementation, including rejected/null results.
        unsafe {
            let c = oracle_obs_data_create_from_json(input.as_ptr());
            let rust = ffi::obs_data_create_from_json(input.as_ptr());
            let expected = consume(c, oracle_obs_data_get_json, oracle_obs_data_release);
            assert_eq!(expected.is_some(), bytes == VALID.as_bytes());
            assert_eq!(
                consume(rust, ffi::obs_data_get_json, ffi::obs_data_release),
                expected
            );
        }
    }
}

#[test]
fn file_json_utf8_matches_c() {
    let dir = common::ScratchDir::new("obs-data-utf8-file-parity");
    let path = dir.join("settings.json");
    let name = path_c(&path);
    for bytes in INVALID
        .iter()
        .copied()
        .chain([VALID.as_bytes(), b"\xef\xbb\xbf{\"ok\":1}"])
    {
        std::fs::write(&path, bytes).unwrap();
        // SAFETY: the valid path outlives both calls; consume releases each result.
        unsafe {
            let c = oracle_obs_data_create_from_json_file(name.as_ptr());
            let rust = ffi::obs_data_create_from_json_file(name.as_ptr());
            let expected = consume(c, oracle_obs_data_get_json, oracle_obs_data_release);
            assert_eq!(expected.is_some(), std::str::from_utf8(bytes).is_ok());
            assert_eq!(
                consume(rust, ffi::obs_data_get_json, ffi::obs_data_release),
                expected
            );
        }
    }
}

#[test]
fn safe_file_utf8_recovery_matches_c() {
    if !common::isolated(concat!(
        module_path!(),
        "::safe_file_utf8_recovery_matches_c"
    )) {
        return;
    }
    let dir = common::ScratchDir::new("obs-data-utf8-parity");
    for bytes in INVALID
        .iter()
        .copied()
        .chain([VALID.as_bytes(), b"\xef\xbb\xbf{\"ok\":1}"])
    {
        for backup in [Some(VALID.as_bytes()), None, Some(INVALID[0])] {
            for ext in [
                c"bak".as_ptr(),
                c".bak".as_ptr(),
                c"".as_ptr(),
                std::ptr::null(),
            ] {
                let c_path = dir.join("c.json");
                let rust_path = dir.join("rust.json");
                for path in [&c_path, &rust_path] {
                    std::fs::write(path, bytes).unwrap();
                    let bak = path.with_extension("json.bak");
                    let _ = std::fs::remove_file(&bak);
                    if let Some(backup) = backup {
                        std::fs::write(bak, backup).unwrap();
                    }
                }
                let c_name = path_c(&c_path);
                let rust_name = path_c(&rust_path);
                // SAFETY: both implementations get separate files with identical
                // bytes and valid paths; consume releases their returned references.
                unsafe {
                    let c = oracle_obs_data_create_from_json_file_safe(c_name.as_ptr(), ext);
                    let rust = ffi::obs_data_create_from_json_file_safe(rust_name.as_ptr(), ext);
                    let expected = consume(c, oracle_obs_data_get_json, oracle_obs_data_release);
                    assert_eq!(
                        consume(rust, ffi::obs_data_get_json, ffi::obs_data_release),
                        expected
                    );
                }
                assert_eq!(
                    std::fs::read(&c_path).unwrap(),
                    std::fs::read(&rust_path).unwrap()
                );
                assert_eq!(
                    c_path.with_extension("json.bak").exists(),
                    rust_path.with_extension("json.bak").exists()
                );
            }
        }
    }
}

#[test]
fn json_null_guards_match_c() {
    // SAFETY: these entry points explicitly permit null receivers/input strings.
    unsafe {
        assert!(oracle_obs_data_create_from_json(std::ptr::null()).is_null());
        assert!(ffi::obs_data_create_from_json(std::ptr::null()).is_null());
        assert!(oracle_obs_data_get_json(std::ptr::null_mut()).is_null());
        assert!(ffi::obs_data_get_json(std::ptr::null_mut()).is_null());
        assert!(!oracle_obs_data_save_json(
            std::ptr::null_mut(),
            std::ptr::null()
        ));
        assert!(!ffi::obs_data_save_json(
            std::ptr::null_mut(),
            std::ptr::null()
        ));
    }
}
