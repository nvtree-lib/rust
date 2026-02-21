#![cfg(target_os = "freebsd")]

use nvtree::{
    nvtree_add, nvtree_add_tree, nvtree_bool, nvtree_create, nvtree_find, nvtree_null,
    nvtree_number, nvtree_pack, nvtree_string, nvtree_tree, nvtree_unpack, Nvtpair, Nvtvalue,
};
use std::ffi::{c_char, c_int, c_void, CString};

#[repr(C)]
struct nvlist_t {
    _private: [u8; 0],
}

#[link(name = "nv")]
unsafe extern "C" {
    #[link_name = "FreeBSD_nvlist_create"]
    fn nvlist_create(flags: c_int) -> *mut nvlist_t;
    #[link_name = "FreeBSD_nvlist_destroy"]
    fn nvlist_destroy(nvl: *mut nvlist_t);

    #[link_name = "FreeBSD_nvlist_add_null"]
    fn nvlist_add_null(nvl: *mut nvlist_t, name: *const c_char);
    #[link_name = "FreeBSD_nvlist_add_bool"]
    fn nvlist_add_bool(nvl: *mut nvlist_t, name: *const c_char, value: bool);
    #[link_name = "FreeBSD_nvlist_add_number"]
    fn nvlist_add_number(nvl: *mut nvlist_t, name: *const c_char, value: u64);
    #[link_name = "FreeBSD_nvlist_add_string"]
    fn nvlist_add_string(nvl: *mut nvlist_t, name: *const c_char, value: *const c_char);
    #[link_name = "FreeBSD_nvlist_add_nvlist"]
    fn nvlist_add_nvlist(nvl: *mut nvlist_t, name: *const c_char, value: *const nvlist_t);

    #[link_name = "FreeBSD_nvlist_pack"]
    fn nvlist_pack(nvl: *const nvlist_t, sizep: *mut usize) -> *mut c_void;
    #[link_name = "FreeBSD_nvlist_unpack"]
    fn nvlist_unpack(buf: *const c_void, size: usize, flags: c_int) -> *mut nvlist_t;

    #[link_name = "FreeBSD_nvlist_exists_null"]
    fn nvlist_exists_null(nvl: *const nvlist_t, name: *const c_char) -> bool;
    #[link_name = "FreeBSD_nvlist_get_bool"]
    fn nvlist_get_bool(nvl: *const nvlist_t, name: *const c_char) -> bool;
    #[link_name = "FreeBSD_nvlist_get_number"]
    fn nvlist_get_number(nvl: *const nvlist_t, name: *const c_char) -> u64;
    #[link_name = "FreeBSD_nvlist_get_string"]
    fn nvlist_get_string(nvl: *const nvlist_t, name: *const c_char) -> *const c_char;
    #[link_name = "FreeBSD_nvlist_get_nvlist"]
    fn nvlist_get_nvlist(nvl: *const nvlist_t, name: *const c_char) -> *const nvlist_t;
}

#[link(name = "c")]
unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

struct NvOwned(*mut nvlist_t);

impl Drop for NvOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer comes from nvlist_create and is owned by this guard.
            unsafe { nvlist_destroy(self.0) };
        }
    }
}

fn cstr(s: &str) -> CString {
    CString::new(s).expect("input must not contain NUL bytes")
}

#[test]
fn unpack_libnv_scalars() {
    // SAFETY: all pointers passed to libnv are valid C strings and live during each call.
    unsafe {
        let root = NvOwned(nvlist_create(0));
        assert!(!root.0.is_null());

        let k_null = cstr("null");
        let k_bool = cstr("bool");
        let k_number = cstr("number");
        let k_string = cstr("string");
        let v_string = cstr("hello-from-libnv");

        nvlist_add_null(root.0, k_null.as_ptr());
        nvlist_add_bool(root.0, k_bool.as_ptr(), true);
        nvlist_add_number(root.0, k_number.as_ptr(), 42);
        nvlist_add_string(root.0, k_string.as_ptr(), v_string.as_ptr());

        let mut packed_size = 0usize;
        let packed_ptr = nvlist_pack(root.0, &mut packed_size as *mut usize);
        assert!(!packed_ptr.is_null());
        assert!(packed_size > 0);

        let packed = std::slice::from_raw_parts(packed_ptr as *const u8, packed_size);
        let tree = nvtree_unpack(packed).expect("nvtree should unpack libnv-packed scalars");

        assert_eq!(
            nvtree_find(&tree, "null").expect("null key should exist").value,
            Nvtvalue::Null
        );
        assert_eq!(
            nvtree_find(&tree, "bool").expect("bool key should exist").value,
            Nvtvalue::Bool(true)
        );
        assert_eq!(
            nvtree_find(&tree, "number")
                .expect("number key should exist")
                .value,
            Nvtvalue::Number(42)
        );
        assert_eq!(
            nvtree_find(&tree, "string")
                .expect("string key should exist")
                .value,
            Nvtvalue::String("hello-from-libnv".to_string())
        );

        free(packed_ptr);
    }
}

#[test]
fn unpack_libnv_nested() {
    // SAFETY: all pointers passed to libnv are valid and point to live objects.
    unsafe {
        let root = NvOwned(nvlist_create(0));
        let child = NvOwned(nvlist_create(0));
        assert!(!root.0.is_null());
        assert!(!child.0.is_null());

        let k_child = cstr("child");
        let k_ok = cstr("ok");
        let k_name = cstr("name");
        let v_name = cstr("inner-libnv");

        nvlist_add_bool(child.0, k_ok.as_ptr(), true);
        nvlist_add_string(child.0, k_name.as_ptr(), v_name.as_ptr());

        nvlist_add_nvlist(root.0, k_child.as_ptr(), child.0);

        let mut packed_size = 0usize;
        let packed_ptr = nvlist_pack(root.0, &mut packed_size as *mut usize);
        assert!(!packed_ptr.is_null());
        assert!(packed_size > 0);

        let packed = std::slice::from_raw_parts(packed_ptr as *const u8, packed_size);
        let tree = nvtree_unpack(packed).expect("nvtree should unpack libnv-packed nested nvlist");

        let child_pair = nvtree_find(&tree, "child").expect("child key should exist");
        match &child_pair.value {
            Nvtvalue::Nested(nested) => {
                assert_eq!(
                    nvtree_find(nested, "ok")
                        .expect("ok should exist in nested nvlist")
                        .value,
                    Nvtvalue::Bool(true)
                );
                assert_eq!(
                    nvtree_find(nested, "name")
                        .expect("name should exist in nested nvlist")
                        .value,
                    Nvtvalue::String("inner-libnv".to_string())
                );
            }
            other => panic!("expected nested value, got {other:?}"),
        }

        free(packed_ptr);
    }
}

#[test]
fn unpack_rust_packed_scalars_with_libnv() {
    // Build with Rust nvtree APIs.
    let mut root = nvtree_create(0);
    nvtree_add(&mut root, nvtree_null("null"));
    nvtree_add(&mut root, nvtree_bool("bool", true));
    nvtree_add(&mut root, nvtree_number("number", 99));
    nvtree_add(&mut root, nvtree_string("string", "hello-from-rust"));

    let packed = nvtree_pack(&root);

    // SAFETY: libnv is called with valid pointers and names that outlive calls.
    unsafe {
        let nvl = NvOwned(nvlist_unpack(packed.as_ptr() as *const c_void, packed.len(), 0));
        assert!(!nvl.0.is_null());

        let k_null = cstr("null");
        let k_bool = cstr("bool");
        let k_number = cstr("number");
        let k_string = cstr("string");
        assert!(nvlist_exists_null(nvl.0, k_null.as_ptr()));
        assert!(nvlist_get_bool(nvl.0, k_bool.as_ptr()));
        assert_eq!(nvlist_get_number(nvl.0, k_number.as_ptr()), 99);

        let cstr_ptr = nvlist_get_string(nvl.0, k_string.as_ptr());
        assert!(!cstr_ptr.is_null());
        let rust_string = std::ffi::CStr::from_ptr(cstr_ptr)
            .to_str()
            .expect("libnv string should be valid utf-8");
        assert_eq!(rust_string, "hello-from-rust");
    }
}

#[test]
fn unpack_rust_packed_nested_with_libnv_get_nvlist() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_tree("child");
    nvtree_add_tree(&mut child, nvtree_bool("ok", true)).expect("nested add should succeed");
    nvtree_add_tree(&mut child, nvtree_string("name", "inner-rust"))
        .expect("nested add should succeed");
    nvtree_add(&mut root, child);

    let packed = nvtree_pack(&root);

    // SAFETY: libnv is called with valid pointers and names that outlive calls.
    unsafe {
        let nvl = NvOwned(nvlist_unpack(packed.as_ptr() as *const c_void, packed.len(), 0));
        assert!(!nvl.0.is_null());

        let k_child = cstr("child");
        let k_ok = cstr("ok");
        let k_name = cstr("name");

        let child_nvl = nvlist_get_nvlist(nvl.0, k_child.as_ptr());
        assert!(!child_nvl.is_null());
        assert!(nvlist_get_bool(child_nvl, k_ok.as_ptr()));

        let child_name_ptr = nvlist_get_string(child_nvl, k_name.as_ptr());
        assert!(!child_name_ptr.is_null());
        let child_name = std::ffi::CStr::from_ptr(child_name_ptr)
            .to_str()
            .expect("nested libnv string should be valid utf-8");
        assert_eq!(child_name, "inner-rust");
    }
}

#[test]
fn array_roundtrip_in_nv_interop_suite() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_create(0);
    nvtree_add(&mut child, nvtree_string("name", "libnv-arr-child"));
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "bools".to_string(),
            value: Nvtvalue::BoolArray(vec![false, true]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "numbers".to_string(),
            value: Nvtvalue::NumberArray(vec![7, 8, 9]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "strings".to_string(),
            value: Nvtvalue::StringArray(vec!["left".to_string(), "right".to_string()]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "trees".to_string(),
            value: Nvtvalue::NestedArray(vec![child]),
        },
    );

    let packed = nvtree_pack(&root);
    let unpacked = nvtree_unpack(&packed).expect("array unpack should succeed");
    assert_eq!(
        nvtree_find(&unpacked, "bools").expect("bools should exist").value,
        Nvtvalue::BoolArray(vec![false, true])
    );
    assert_eq!(
        nvtree_find(&unpacked, "numbers")
            .expect("numbers should exist")
            .value,
        Nvtvalue::NumberArray(vec![7, 8, 9])
    );
    assert_eq!(
        nvtree_find(&unpacked, "strings")
            .expect("strings should exist")
            .value,
        Nvtvalue::StringArray(vec!["left".to_string(), "right".to_string()])
    );
}
