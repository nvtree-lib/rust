#![cfg(target_os = "freebsd")]

use nvtree::{
    nvtree_add, nvtree_add_tree, nvtree_bool, nvtree_create, nvtree_find, nvtree_null,
    nvtree_number, nvtree_pack, nvtree_string, nvtree_tree, nvtree_unpack, Nvtpair, Nvtvalue,
};
use std::ffi::c_void;

const NVTREE_FLAG_BIG_ENDIAN: u8 = 0x80;
const NV_TYPE_NULL: u8 = 1;
const NV_TYPE_BOOL: u8 = 2;
const NV_TYPE_NUMBER: u8 = 3;
const NV_TYPE_STRING: u8 = 4;
const TREE_HEADER_LEN: usize = 19;
const PAIR_HEADER_LEN: usize = 19;

unsafe extern "C" {
    fn cpp_nvtpp_pack_scalars(out_buf: *mut *mut u8, out_len: *mut usize) -> i32;
    fn cpp_nvtpp_pack_nested(out_buf: *mut *mut u8, out_len: *mut usize) -> i32;
    fn cpp_nvtpp_unpack_validate_scalars(buf: *const u8, len: usize) -> i32;
    fn cpp_nvtpp_unpack_validate_nested(buf: *const u8, len: usize) -> i32;
    fn free(ptr: *mut c_void);
}

fn read_u16_raw(buf: &[u8], off: usize, is_be: bool) -> u16 {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&buf[off..off + 2]);
    if is_be {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}

fn read_u64_raw(buf: &[u8], off: usize, is_be: bool) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[off..off + 8]);
    if is_be {
        u64::from_be_bytes(bytes)
    } else {
        u64::from_le_bytes(bytes)
    }
}

fn write_u16_raw(buf: &mut [u8], off: usize, value: u16, is_be: bool) {
    let bytes = if is_be {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    buf[off..off + 2].copy_from_slice(&bytes);
}

fn write_u64_raw(buf: &mut [u8], off: usize, value: u64, is_be: bool) {
    let bytes = if is_be {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    buf[off..off + 8].copy_from_slice(&bytes);
}

fn rewrite_simple_tree_endian(buf: &mut [u8], src_is_be: bool, dst_is_be: bool) -> Result<(), ()> {
    if buf.len() < TREE_HEADER_LEN {
        return Err(());
    }

    buf[2] &= !NVTREE_FLAG_BIG_ENDIAN;
    buf[2] |= if dst_is_be { NVTREE_FLAG_BIG_ENDIAN } else { 0 };
    write_u64_raw(buf, 3, read_u64_raw(buf, 3, src_is_be), dst_is_be);
    write_u64_raw(buf, 11, read_u64_raw(buf, 11, src_is_be), dst_is_be);

    let mut ptr = TREE_HEADER_LEN;
    while ptr + PAIR_HEADER_LEN <= buf.len() {
        let ty = buf[ptr];
        let namesize = read_u16_raw(buf, ptr + 1, src_is_be);
        let datasize = read_u64_raw(buf, ptr + 3, src_is_be);
        let nitems = read_u64_raw(buf, ptr + 11, src_is_be);
        write_u16_raw(buf, ptr + 1, namesize, dst_is_be);
        write_u64_raw(buf, ptr + 3, datasize, dst_is_be);
        write_u64_raw(buf, ptr + 11, nitems, dst_is_be);
        ptr += PAIR_HEADER_LEN;
        let ns = namesize as usize;
        if ns == 0 || ptr + ns > buf.len() {
            return Err(());
        }
        ptr += ns;

        match ty {
            NV_TYPE_NULL => {}
            NV_TYPE_BOOL => {
                if ptr + 1 > buf.len() {
                    return Err(());
                }
                ptr += 1;
            }
            NV_TYPE_NUMBER => {
                if ptr + 8 > buf.len() {
                    return Err(());
                }
                write_u64_raw(buf, ptr, read_u64_raw(buf, ptr, src_is_be), dst_is_be);
                ptr += 8;
            }
            NV_TYPE_STRING => {
                let ds = datasize as usize;
                if ptr + ds > buf.len() {
                    return Err(());
                }
                ptr += ds;
            }
            _ => return Err(()),
        }
    }

    if ptr != buf.len() {
        return Err(());
    }
    Ok(())
}

fn unpack_from_cpp_packer(
    f: unsafe extern "C" fn(out_buf: *mut *mut u8, out_len: *mut usize) -> i32,
) -> nvtree::Nvtree {
    // SAFETY: helper allocates output with malloc and returns valid pointer/length on success.
    unsafe {
        let mut packed_ptr = std::ptr::null_mut::<u8>();
        let mut packed_len = 0usize;
        let ok = f(&mut packed_ptr as *mut *mut u8, &mut packed_len as *mut usize);
        assert_eq!(ok, 1, "C++ helper should pack successfully");
        assert!(!packed_ptr.is_null(), "C++ helper should return non-null packed buffer");
        assert!(packed_len > 0, "C++ helper should return non-zero packed size");

        let packed = std::slice::from_raw_parts(packed_ptr as *const u8, packed_len);
        let tree = nvtree_unpack(packed).expect("Rust nvtree should unpack C++-packed buffer");
        free(packed_ptr as *mut c_void);
        tree
    }
}

#[test]
fn unpack_cpp_packed_scalars_in_rust() {
    let tree = unpack_from_cpp_packer(cpp_nvtpp_pack_scalars);

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
        Nvtvalue::String("hello-from-cpp".to_string())
    );
}

#[test]
fn unpack_cpp_packed_nested_in_rust() {
    let tree = unpack_from_cpp_packer(cpp_nvtpp_pack_nested);

    let child = nvtree_find(&tree, "child").expect("child key should exist");
    match &child.value {
        Nvtvalue::Nested(nested) => {
            assert_eq!(
                nvtree_find(nested, "ok")
                    .expect("ok should exist in nested tree")
                    .value,
                Nvtvalue::Bool(true)
            );
            assert_eq!(
                nvtree_find(nested, "name")
                    .expect("name should exist in nested tree")
                    .value,
                Nvtvalue::String("inner-cpp".to_string())
            );
        }
        other => panic!("expected nested value, got {other:?}"),
    }
}

#[test]
fn unpack_rust_packed_scalars_in_cpp() {
    let mut root = nvtree_create(0);
    nvtree_add(&mut root, nvtree_null("null"));
    nvtree_add(&mut root, nvtree_bool("bool", true));
    nvtree_add(&mut root, nvtree_number("number", 99));
    nvtree_add(&mut root, nvtree_string("string", "hello-from-rust-to-cpp"));
    let packed = nvtree_pack(&root);

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { cpp_nvtpp_unpack_validate_scalars(packed.as_ptr(), packed.len()) };
    assert_eq!(ok, 1, "C++ nvtpp should unpack and validate Rust-packed scalars");
}

#[test]
fn unpack_rust_packed_nested_in_cpp() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_tree("child");
    nvtree_add_tree(&mut child, nvtree_bool("ok", true)).expect("nested add should succeed");
    nvtree_add_tree(&mut child, nvtree_string("name", "inner-rust-to-cpp"))
        .expect("nested add should succeed");
    nvtree_add(&mut root, child);
    let packed = nvtree_pack(&root);

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { cpp_nvtpp_unpack_validate_nested(packed.as_ptr(), packed.len()) };
    assert_eq!(ok, 1, "C++ nvtpp should unpack and validate Rust-packed nested tree");
}

#[test]
fn unpack_endian_swapped_rust_packed_scalars_in_cpp() {
    let mut root = nvtree_create(0);
    nvtree_add(&mut root, nvtree_null("null"));
    nvtree_add(&mut root, nvtree_bool("bool", true));
    nvtree_add(&mut root, nvtree_number("number", 99));
    nvtree_add(&mut root, nvtree_string("string", "hello-from-rust-to-cpp"));
    let mut packed = nvtree_pack(&root);

    let src_is_be = cfg!(target_endian = "big");
    let dst_is_be = !src_is_be;
    rewrite_simple_tree_endian(&mut packed, src_is_be, dst_is_be)
        .expect("endianness rewrite should succeed");

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { cpp_nvtpp_unpack_validate_scalars(packed.as_ptr(), packed.len()) };
    assert_eq!(
        ok, 1,
        "C++ nvtpp should unpack and validate Rust-packed opposite-endian scalars"
    );
}

#[test]
fn array_roundtrip_in_nvtpp_interop_suite() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_create(0);
    nvtree_add(&mut child, nvtree_string("name", "cpp-arr-child"));
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "bools".to_string(),
            value: Nvtvalue::BoolArray(vec![true, false, true]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "numbers".to_string(),
            value: Nvtvalue::NumberArray(vec![10, 20]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "strings".to_string(),
            value: Nvtvalue::StringArray(vec!["x".to_string(), "y".to_string()]),
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
        Nvtvalue::BoolArray(vec![true, false, true])
    );
    assert_eq!(
        nvtree_find(&unpacked, "numbers")
            .expect("numbers should exist")
            .value,
        Nvtvalue::NumberArray(vec![10, 20])
    );
    assert_eq!(
        nvtree_find(&unpacked, "strings")
            .expect("strings should exist")
            .value,
        Nvtvalue::StringArray(vec!["x".to_string(), "y".to_string()])
    );
}
