//! Callback-time state must match the unchanged C properties implementation.

use std::ffi::c_void;

use obs_c_oracle::obs_properties as c;
use obs_properties::ffi::obs_properties as rs;

type GetParam = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type SetParam =
    unsafe extern "C" fn(*mut c_void, *mut c_void, Option<unsafe extern "C" fn(*mut c_void)>);
type SetFlags = unsafe extern "C" fn(*mut c_void, u32);

struct Param {
    props: *mut c_void,
    get: GetParam,
    flags: SetFlags,
    saw_self: bool,
    calls: u32,
}

unsafe extern "C" fn destroy(param: *mut c_void) {
    // SAFETY: the test keeps this Param and its properties alive through the callback.
    unsafe {
        let state = &mut *param.cast::<Param>();
        state.saw_self = (state.get)(state.props) == param;
        state.calls += 1;
        (state.flags)(state.props, 42);
    }
}

#[test]
fn replacement_destructor_observes_old_param_like_c() {
    for clear in [false, true] {
        for oracle in [true, false] {
            let (create, set, get, flags, get_flags, free) = if oracle {
                (
                    c::oracle_obs_properties_create as unsafe extern "C" fn() -> *mut c_void,
                    c::oracle_obs_properties_set_param as SetParam,
                    c::oracle_obs_properties_get_param as GetParam,
                    c::oracle_obs_properties_set_flags as SetFlags,
                    c::oracle_obs_properties_get_flags as unsafe extern "C" fn(_) -> u32,
                    c::oracle_obs_properties_destroy as unsafe extern "C" fn(_),
                )
            } else {
                (
                    rs::obs_properties_create as unsafe extern "C" fn() -> *mut c_void,
                    rs::obs_properties_set_param as SetParam,
                    rs::obs_properties_get_param as GetParam,
                    rs::obs_properties_set_flags as SetFlags,
                    rs::obs_properties_get_flags as unsafe extern "C" fn(_) -> u32,
                    rs::obs_properties_destroy as unsafe extern "C" fn(_),
                )
            };
            // SAFETY: all handles and callback parameters remain live until free returns.
            unsafe {
                let props = create();
                let mut old = Param {
                    props,
                    get,
                    flags,
                    saw_self: false,
                    calls: 0,
                };
                let mut new = Param {
                    props,
                    get,
                    flags,
                    saw_self: false,
                    calls: 0,
                };
                set(props, (&raw mut old).cast(), Some(destroy));
                let replacement = if clear {
                    std::ptr::null_mut()
                } else {
                    (&raw mut new).cast()
                };
                set(props, replacement, Some(destroy));
                let observed = (
                    old.saw_self,
                    old.calls,
                    get(props) == replacement,
                    get_flags(props),
                );
                free(props);
                assert_eq!(
                    observed,
                    (true, 1, true, 42),
                    "oracle={oracle}, clear={clear}"
                );
                assert_eq!(new.calls, u32::from(!clear));
                assert_eq!(new.saw_self, !clear);
            }
        }
    }
}
