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
const NV_TYPE_NVLIST: u8 = 5;
const NV_TYPE_END: u8 = 0xff;
const TREE_HEADER_LEN: usize = 19;
const PAIR_HEADER_LEN: usize = 19;

unsafe extern "C" {
    fn c_nvtree_pack_scalars(out_buf: *mut *mut u8, out_len: *mut usize) -> i32;
    fn c_nvtree_pack_nested(out_buf: *mut *mut u8, out_len: *mut usize) -> i32;
    fn c_nvtree_unpack_validate_scalars(buf: *const u8, len: usize) -> i32;
    fn c_nvtree_unpack_validate_nested(buf: *const u8, len: usize) -> i32;
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

fn rewrite_tree_endian(
    buf: &mut [u8],
    start: usize,
    src_is_be: bool,
    dst_is_be: bool,
    root: bool,
) -> Result<usize, ()> {
    if buf.len().saturating_sub(start) < TREE_HEADER_LEN {
        return Err(());
    }

    let flags = buf[start + 2] & !NVTREE_FLAG_BIG_ENDIAN;
    buf[start + 2] = flags | if dst_is_be { NVTREE_FLAG_BIG_ENDIAN } else { 0 };

    let desc = read_u64_raw(buf, start + 3, src_is_be);
    let size = read_u64_raw(buf, start + 11, src_is_be);
    write_u64_raw(buf, start + 3, desc, dst_is_be);
    write_u64_raw(buf, start + 11, size, dst_is_be);

    let body_len = if root {
        size as usize
    } else {
        size.checked_sub((TREE_HEADER_LEN + 1) as u64).ok_or(())? as usize
    };
    let mut ptr = start + TREE_HEADER_LEN;
    let body_end = ptr.checked_add(body_len).ok_or(())?;
    if body_end > buf.len() {
        return Err(());
    }

    while ptr < body_end {
        let ty = buf[ptr];
        let namesize = read_u16_raw(buf, ptr + 1, src_is_be);
        let datasize = read_u64_raw(buf, ptr + 3, src_is_be);
        let nitems = read_u64_raw(buf, ptr + 11, src_is_be);
        write_u16_raw(buf, ptr + 1, namesize, dst_is_be);
        write_u64_raw(buf, ptr + 3, datasize, dst_is_be);
        write_u64_raw(buf, ptr + 11, nitems, dst_is_be);

        ptr += PAIR_HEADER_LEN;
        let ns = namesize as usize;
        if ns == 0 || ptr + ns > body_end {
            return Err(());
        }
        ptr += ns;

        match ty {
            NV_TYPE_NULL => {}
            NV_TYPE_BOOL => {
                if ptr + 1 > body_end {
                    return Err(());
                }
                ptr += 1;
            }
            NV_TYPE_NUMBER => {
                if ptr + 8 > body_end {
                    return Err(());
                }
                let num = read_u64_raw(buf, ptr, src_is_be);
                write_u64_raw(buf, ptr, num, dst_is_be);
                ptr += 8;
            }
            NV_TYPE_STRING => {
                let ds = datasize as usize;
                if ptr + ds > body_end {
                    return Err(());
                }
                ptr += ds;
            }
            NV_TYPE_NVLIST => {
                let ds = datasize as usize;
                let consumed = rewrite_tree_endian(buf, ptr, src_is_be, dst_is_be, false)?;
                if consumed != ds {
                    return Err(());
                }
                ptr += consumed;
                if ptr + PAIR_HEADER_LEN + 1 <= body_end && buf[ptr] == NV_TYPE_END {
                    let marker_namesize = read_u16_raw(buf, ptr + 1, src_is_be);
                    let marker_datasize = read_u64_raw(buf, ptr + 3, src_is_be);
                    let marker_nitems = read_u64_raw(buf, ptr + 11, src_is_be);
                    write_u16_raw(buf, ptr + 1, marker_namesize, dst_is_be);
                    write_u64_raw(buf, ptr + 3, marker_datasize, dst_is_be);
                    write_u64_raw(buf, ptr + 11, marker_nitems, dst_is_be);
                    ptr += PAIR_HEADER_LEN + 1;
                }
            }
            NV_TYPE_END => break,
            _ => return Err(()),
        }
    }

    Ok(TREE_HEADER_LEN + body_len)
}

fn c_pack_bytes(
    f: unsafe extern "C" fn(out_buf: *mut *mut u8, out_len: *mut usize) -> i32,
) -> Vec<u8> {
    // SAFETY: helper allocates output with malloc and returns valid pointer/length on success.
    unsafe {
        let mut packed_ptr = std::ptr::null_mut::<u8>();
        let mut packed_len = 0usize;
        let ok = f(&mut packed_ptr as *mut *mut u8, &mut packed_len as *mut usize);
        assert_eq!(ok, 1, "C helper should pack successfully");
        assert!(!packed_ptr.is_null(), "C helper should return non-null packed buffer");
        assert!(packed_len > 0, "C helper should return non-zero packed size");
        let packed = std::slice::from_raw_parts(packed_ptr as *const u8, packed_len).to_vec();
        free(packed_ptr as *mut c_void);
        packed
    }
}

fn unpack_from_c_packer(
    f: unsafe extern "C" fn(out_buf: *mut *mut u8, out_len: *mut usize) -> i32,
) -> nvtree::Nvtree {
    // SAFETY: helper allocates output with malloc and returns valid pointer/length on success.
    unsafe {
        let mut packed_ptr = std::ptr::null_mut::<u8>();
        let mut packed_len = 0usize;
        let ok = f(&mut packed_ptr as *mut *mut u8, &mut packed_len as *mut usize);
        assert_eq!(ok, 1, "C helper should pack successfully");
        assert!(!packed_ptr.is_null(), "C helper should return non-null packed buffer");
        assert!(packed_len > 0, "C helper should return non-zero packed size");

        let packed = std::slice::from_raw_parts(packed_ptr as *const u8, packed_len);
        let tree = nvtree_unpack(packed).expect("Rust nvtree should unpack C-packed buffer");
        free(packed_ptr as *mut c_void);
        tree
    }
}

#[test]
fn unpack_c_packed_scalars_in_rust() {
    let tree = unpack_from_c_packer(c_nvtree_pack_scalars);

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
        Nvtvalue::String("hello-from-c".to_string())
    );
}

#[test]
fn unpack_c_packed_nested_in_rust() {
    let tree = unpack_from_c_packer(c_nvtree_pack_nested);

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
                Nvtvalue::String("inner-c".to_string())
            );
        }
        other => panic!("expected nested value, got {other:?}"),
    }
}

#[test]
fn unpack_rust_packed_scalars_in_c() {
    let mut root = nvtree_create(0);
    nvtree_add(&mut root, nvtree_null("null"));
    nvtree_add(&mut root, nvtree_bool("bool", true));
    nvtree_add(&mut root, nvtree_number("number", 99));
    nvtree_add(&mut root, nvtree_string("string", "hello-from-rust-to-c"));
    let packed = nvtree_pack(&root);

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { c_nvtree_unpack_validate_scalars(packed.as_ptr(), packed.len()) };
    assert_eq!(ok, 1, "C nvtree should unpack and validate Rust-packed scalars");
}

#[test]
fn unpack_endian_swapped_rust_packed_scalars_in_c() {
    let mut root = nvtree_create(0);
    nvtree_add(&mut root, nvtree_null("null"));
    nvtree_add(&mut root, nvtree_bool("bool", true));
    nvtree_add(&mut root, nvtree_number("number", 99));
    nvtree_add(&mut root, nvtree_string("string", "hello-from-rust-to-c"));
    let mut packed = nvtree_pack(&root);

    let src_is_be = cfg!(target_endian = "big");
    let dst_is_be = !src_is_be;
    rewrite_tree_endian(&mut packed, 0, src_is_be, dst_is_be, true)
        .expect("endianness rewrite should succeed");

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { c_nvtree_unpack_validate_scalars(packed.as_ptr(), packed.len()) };
    assert_eq!(
        ok, 1,
        "C nvtree should unpack and validate Rust-packed opposite-endian scalars"
    );
}

#[test]
fn unpack_rust_packed_nested_in_c() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_tree("child");
    nvtree_add_tree(&mut child, nvtree_bool("ok", true)).expect("nested add should succeed");
    nvtree_add_tree(&mut child, nvtree_string("name", "inner-rust-to-c"))
        .expect("nested add should succeed");
    nvtree_add(&mut root, child);
    let packed = nvtree_pack(&root);

    // SAFETY: packed buffer pointer/len are valid for the duration of the call.
    let ok = unsafe { c_nvtree_unpack_validate_nested(packed.as_ptr(), packed.len()) };
    assert_eq!(ok, 1, "C nvtree should unpack and validate Rust-packed nested tree");
}

#[test]
fn unpack_endian_swapped_c_packed_scalars_in_rust() {
    let mut packed = c_pack_bytes(c_nvtree_pack_scalars);
    let src_is_be = cfg!(target_endian = "big");
    let dst_is_be = !src_is_be;
    rewrite_tree_endian(&mut packed, 0, src_is_be, dst_is_be, true)
        .expect("endianness rewrite should succeed");

    let tree = nvtree_unpack(&packed).expect("Rust nvtree should unpack endian-marked C buffer");
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
        Nvtvalue::String("hello-from-c".to_string())
    );
}

#[test]
fn array_roundtrip_in_nvtree_interop_suite() {
    let mut root = nvtree_create(0);
    let mut child = nvtree_create(0);
    nvtree_add(&mut child, nvtree_string("name", "arr-child"));
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "bools".to_string(),
            value: Nvtvalue::BoolArray(vec![true, false]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "numbers".to_string(),
            value: Nvtvalue::NumberArray(vec![1, 2, 3]),
        },
    );
    nvtree_add(
        &mut root,
        Nvtpair {
            flags: 0,
            name: "strings".to_string(),
            value: Nvtvalue::StringArray(vec!["a".to_string(), "b".to_string()]),
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
        Nvtvalue::BoolArray(vec![true, false])
    );
    assert_eq!(
        nvtree_find(&unpacked, "numbers")
            .expect("numbers should exist")
            .value,
        Nvtvalue::NumberArray(vec![1, 2, 3])
    );
    assert_eq!(
        nvtree_find(&unpacked, "strings")
            .expect("strings should exist")
            .value,
        Nvtvalue::StringArray(vec!["a".to_string(), "b".to_string()])
    );
}
