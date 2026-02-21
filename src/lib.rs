use std::collections::BTreeMap;
#[cfg(target_os = "freebsd")]
use std::ffi::{CStr, c_char, c_int, c_void};

pub const NVTREE_RO: u8 = 0x001;
pub const NVTREE_NODELETE: u8 = 0x002;

pub const NVTREE_BOOL: u16 = 0x010;
pub const NVTREE_NUMBER: u16 = 0x020;
pub const NVTREE_STRING: u16 = 0x040;
pub const NVTREE_NULL: u16 = 0x080;
pub const NVTREE_SIMPLE: u16 = NVTREE_BOOL | NVTREE_NUMBER | NVTREE_STRING | NVTREE_NULL;

pub const NVTREE_ARRAY: u16 = 0x100;
pub const NVTREE_NESTED: u16 = 0x200;

const NVTREE_HEADER_MAGIC: u8 = 0x6c;
const NVTREE_HEADER_VERSION: u8 = 0x00;
const NVTREE_FLAG_LITTLE_ENDIAN: u8 = 0x00;
const NVTREE_FLAG_BIG_ENDIAN: u8 = 0x80;

const NV_TYPE_NULL: u8 = 1;
const NV_TYPE_BOOL: u8 = 2;
const NV_TYPE_NUMBER: u8 = 3;
const NV_TYPE_STRING: u8 = 4;
const NV_TYPE_NVLIST: u8 = 5;
const NV_TYPE_BOOL_ARRAY: u8 = 8;
const NV_TYPE_NUMBER_ARRAY: u8 = 9;
const NV_TYPE_STRING_ARRAY: u8 = 10;
const NV_TYPE_NVLIST_ARRAY: u8 = 11;
const NV_TYPE_END: u8 = 0xff;

const TREE_HEADER_LEN: usize = 19;
const PAIR_HEADER_LEN: usize = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvtreeError {
    BufferTooSmall,
    InvalidMagic,
    InvalidVersion,
    InvalidUtf8,
    InvalidName,
    UnsupportedType(u8),
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nvtvalue {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Nested(Box<Nvtree>),
    BoolArray(Vec<bool>),
    NumberArray(Vec<u64>),
    StringArray(Vec<String>),
    NestedArray(Vec<Nvtree>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvtpair {
    pub flags: u8,
    pub name: String,
    pub value: Nvtvalue,
}

impl Nvtpair {
    pub fn kind(&self) -> u16 {
        match self.value {
            Nvtvalue::Null => NVTREE_NULL,
            Nvtvalue::Bool(_) => NVTREE_BOOL,
            Nvtvalue::Number(_) => NVTREE_NUMBER,
            Nvtvalue::String(_) => NVTREE_STRING,
            Nvtvalue::Nested(_) => NVTREE_NESTED,
            Nvtvalue::BoolArray(_) => NVTREE_ARRAY | NVTREE_BOOL,
            Nvtvalue::NumberArray(_) => NVTREE_ARRAY | NVTREE_NUMBER,
            Nvtvalue::StringArray(_) => NVTREE_ARRAY | NVTREE_STRING,
            Nvtvalue::NestedArray(_) => NVTREE_ARRAY | NVTREE_NESTED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvtree {
    pub flags: u8,
    head: BTreeMap<String, Nvtpair>,
}


pub fn nvtree_create(flags: u8) -> Nvtree {
    Nvtree {
        flags,
        head: BTreeMap::new(),
    }
}

pub fn nvtree_pair(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Null,
    }
}

pub fn nvtree_number(name: &str, value: u64) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Number(value),
    }
}

pub fn nvtree_bool(name: &str, value: bool) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Bool(value),
    }
}

pub fn nvtree_string(name: &str, value: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::String(value.to_string()),
    }
}

pub fn nvtree_null(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Null,
    }
}

pub fn nvtree_tree(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Nested(Box::new(nvtree_create(0))),
    }
}

pub fn nvtree_nested(name: &str, flags: u8) -> Nvtpair {
    Nvtpair {
        flags,
        name: name.to_string(),
        value: Nvtvalue::Nested(Box::new(nvtree_create(flags))),
    }
}

pub fn nvtree_find<'a>(root: &'a Nvtree, name: &str) -> Option<&'a Nvtpair> {
    root.head.get(name)
}

pub fn nvtree_add(root: &mut Nvtree, pair: Nvtpair) -> Option<Nvtpair> {
    root.head.insert(pair.name.clone(), pair)
}

pub fn nvtree_remove(root: &mut Nvtree, name: &str) -> Option<Nvtpair> {
    root.head.remove(name)
}

pub fn nvtree_add_tree(tree: &mut Nvtpair, pair: Nvtpair) -> Result<Option<Nvtpair>, NvtreeError> {
    match &mut tree.value {
        Nvtvalue::Nested(nested) => Ok(nested.head.insert(pair.name.clone(), pair)),
        _ => Err(NvtreeError::Malformed),
    }
}

pub fn nvtree_rem_tree(tree: &mut Nvtpair, name: &str) -> Result<Option<Nvtpair>, NvtreeError> {
    match &mut tree.value {
        Nvtvalue::Nested(nested) => Ok(nested.head.remove(name)),
        _ => Err(NvtreeError::Malformed),
    }
}

pub fn nvtree_size(root: &Nvtree) -> usize {
    nvtree_pack(root).len()
}

pub fn nvtree_pack(root: &Nvtree) -> Vec<u8> {
    serialize_tree(root, true)
}

pub fn nvtree_unpack(buf: &[u8]) -> Result<Nvtree, NvtreeError> {
    match parse_tree(buf, 0, true) {
        Ok((tree, consumed)) => {
            if consumed > buf.len() {
                return Err(NvtreeError::Malformed);
            }
            Ok(tree)
        }
        #[cfg(target_os = "freebsd")]
        Err(err) => {
            // FreeBSD kernel nvlist streams may use encodings not covered by
            // the pure parser. Fall back to libnv decode for compatibility.
            freebsd_unpack_with_libnv(buf).or(Err(err))
        }
        #[cfg(not(target_os = "freebsd"))]
        Err(err) => Err(err),
    }
}

pub fn nvtree_destroy(_root: Nvtree) -> i32 {
    0
}

fn serialize_tree(tree: &Nvtree, root: bool) -> Vec<u8> {
    let byte_order = host_byte_order();
    let mut body = Vec::new();
    for pair in tree.head.values() {
        serialize_pair(pair, &mut body, byte_order);
    }

    let total_size = TREE_HEADER_LEN + body.len();
    let size_field = if root {
        body.len() as u64
    } else {
        (total_size + 1) as u64
    };

    let mut out = Vec::with_capacity(total_size);
    out.push(NVTREE_HEADER_MAGIC);
    out.push(NVTREE_HEADER_VERSION);
    let flags_wire = (tree.flags & !NVTREE_FLAG_BIG_ENDIAN)
        | if byte_order == ByteOrder::Big {
            NVTREE_FLAG_BIG_ENDIAN
        } else {
            NVTREE_FLAG_LITTLE_ENDIAN
        };
    out.push(flags_wire);
    write_u64(&mut out, 0u64, byte_order);
    write_u64(&mut out, size_field, byte_order);
    out.extend_from_slice(&body);
    out
}

fn serialize_pair(pair: &Nvtpair, out: &mut Vec<u8>, byte_order: ByteOrder) {
    let mut name = pair.name.as_bytes().to_vec();
    name.push(0);
    let namesize = u16::try_from(name.len()).unwrap_or(u16::MAX);

    match &pair.value {
        Nvtvalue::Null => {
            write_pair_header(out, NV_TYPE_NULL, namesize, 0, 0, byte_order);
            out.extend_from_slice(&name);
        }
        Nvtvalue::Bool(v) => {
            write_pair_header(out, NV_TYPE_BOOL, namesize, 1, 0, byte_order);
            out.extend_from_slice(&name);
            out.push(u8::from(*v));
        }
        Nvtvalue::Number(v) => {
            write_pair_header(out, NV_TYPE_NUMBER, namesize, 8, 0, byte_order);
            out.extend_from_slice(&name);
            write_u64(out, *v, byte_order);
        }
        Nvtvalue::String(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            write_pair_header(
                out,
                NV_TYPE_STRING,
                namesize,
                bytes.len() as u64,
                0,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
        Nvtvalue::Nested(tree) => {
            let nested = serialize_tree(tree, false);
            write_pair_header(
                out,
                NV_TYPE_NVLIST,
                namesize,
                nested.len() as u64,
                0,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&nested);

            // C implementation appends an explicit END marker after each nested nvlist.
            write_pair_header(out, NV_TYPE_END, 1, 0, 0, byte_order);
            out.push(0);
        }
        Nvtvalue::BoolArray(values) => {
            write_pair_header(
                out,
                NV_TYPE_BOOL_ARRAY,
                namesize,
                values.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend(values.iter().map(|v| u8::from(*v)));
        }
        Nvtvalue::NumberArray(values) => {
            write_pair_header(
                out,
                NV_TYPE_NUMBER_ARRAY,
                namesize,
                (values.len() * 8) as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            for v in values {
                write_u64(out, *v, byte_order);
            }
        }
        Nvtvalue::StringArray(values) => {
            let mut bytes = Vec::new();
            for value in values {
                bytes.extend_from_slice(value.as_bytes());
                bytes.push(0);
            }
            write_pair_header(
                out,
                NV_TYPE_STRING_ARRAY,
                namesize,
                bytes.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
        Nvtvalue::NestedArray(values) => {
            let mut bytes = Vec::new();
            for value in values {
                bytes.extend_from_slice(&serialize_tree(value, false));
                write_pair_header(&mut bytes, NV_TYPE_END, 1, 0, 0, byte_order);
                bytes.push(0);
            }
            write_pair_header(
                out,
                NV_TYPE_NVLIST_ARRAY,
                namesize,
                bytes.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
    }
}

fn write_pair_header(
    out: &mut Vec<u8>,
    ty: u8,
    namesize: u16,
    datasize: u64,
    nitems: u64,
    byte_order: ByteOrder,
) {
    out.push(ty);
    write_u16(out, namesize, byte_order);
    write_u64(out, datasize, byte_order);
    write_u64(out, nitems, byte_order);
}

fn parse_tree(buf: &[u8], start: usize, root: bool) -> Result<(Nvtree, usize), NvtreeError> {
    if buf.len().saturating_sub(start) < TREE_HEADER_LEN {
        return Err(NvtreeError::BufferTooSmall);
    }

    let magic = buf[start];
    let version = buf[start + 1];
    let flags_wire = buf[start + 2];
    let byte_order = if (flags_wire & NVTREE_FLAG_BIG_ENDIAN) != 0 {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    };
    let flags = flags_wire & !NVTREE_FLAG_BIG_ENDIAN;

    if magic != NVTREE_HEADER_MAGIC {
        return Err(NvtreeError::InvalidMagic);
    }
    if version != NVTREE_HEADER_VERSION {
        return Err(NvtreeError::InvalidVersion);
    }

    let mut off = start + 3 + 8;
    let size = read_u64(buf, &mut off, byte_order)? as usize;

    let body_len = if root {
        size
    } else {
        // FreeBSD nvlist streams are observed in two nested-size encodings:
        // - size includes header + body + trailing NUL
        // - size includes header + body
        // Accept both to interoperate with kernel/userland producers.
        size.checked_sub(TREE_HEADER_LEN + 1)
            .or_else(|| size.checked_sub(TREE_HEADER_LEN))
            .ok_or(NvtreeError::Malformed)?
    };

    let mut ptr = start + TREE_HEADER_LEN;
    let body_end = ptr.checked_add(body_len).ok_or(NvtreeError::Malformed)?;
    if body_end > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }

    let mut tree = nvtree_create(flags);

    while ptr < body_end {
        let ty = read_u8(buf, &mut ptr)?;
        let namesize = read_u16(buf, &mut ptr, byte_order)? as usize;
        let datasize = read_u64(buf, &mut ptr, byte_order)? as usize;
        let nitems = read_u64(buf, &mut ptr, byte_order)? as usize;

        if namesize == 0 || ptr + namesize > body_end {
            return Err(NvtreeError::InvalidName);
        }

        let name_raw = &buf[ptr..ptr + namesize - 1];
        ptr += namesize;
        let name = std::str::from_utf8(name_raw)
            .map_err(|_| NvtreeError::InvalidUtf8)?
            .to_string();

        let pair = match ty {
            NV_TYPE_NULL => nvtree_null(&name),
            NV_TYPE_BOOL => {
                if ptr + 1 > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let v = buf[ptr] != 0;
                ptr += 1;
                nvtree_bool(&name, v)
            }
            NV_TYPE_NUMBER => {
                if ptr + 8 > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&buf[ptr..ptr + 8]);
                ptr += 8;
                let v = match byte_order {
                    ByteOrder::Little => u64::from_le_bytes(bytes),
                    ByteOrder::Big => u64::from_be_bytes(bytes),
                };
                nvtree_number(&name, v)
            }
            NV_TYPE_STRING => {
                if ptr + datasize > body_end || datasize == 0 {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let str_raw = &buf[ptr..ptr + datasize - 1];
                let s = std::str::from_utf8(str_raw).map_err(|_| NvtreeError::InvalidUtf8)?;
                ptr += datasize;
                nvtree_string(&name, s)
            }
            NV_TYPE_NVLIST => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let (nested, consumed) = parse_tree(buf, ptr, false)?;
                if consumed > datasize {
                    return Err(NvtreeError::Malformed);
                }
                ptr += consumed;

                // Some streams encode nested size without marker/terminator bytes.
                // Consume any trailing bytes that are accounted for by datasize.
                if consumed < datasize {
                    let rem = datasize - consumed;
                    if rem == 1 && ptr < body_end {
                        ptr += 1;
                    } else if rem == PAIR_HEADER_LEN + 1 && ptr + rem <= body_end && buf[ptr] == NV_TYPE_END {
                        ptr += rem;
                    } else {
                        return Err(NvtreeError::Malformed);
                    }
                } else if ptr + PAIR_HEADER_LEN + 1 <= body_end && buf[ptr] == NV_TYPE_END {
                    // C implementation may append an explicit END marker after nested nvlists.
                    ptr += PAIR_HEADER_LEN + 1;
                }

                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::Nested(Box::new(nested)),
                }
            }
            NV_TYPE_BOOL_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if nitems > datasize {
                    return Err(NvtreeError::Malformed);
                }
                let values = buf[ptr..ptr + nitems]
                    .iter()
                    .map(|b| *b != 0)
                    .collect::<Vec<_>>();
                ptr += datasize;
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::BoolArray(values),
                }
            }
            NV_TYPE_NUMBER_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if datasize % 8 != 0 || nitems > (datasize / 8) {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    let v = read_u64(buf, &mut ptr, byte_order)?;
                    values.push(v);
                }
                ptr += datasize - (nitems * 8);
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NumberArray(values),
                }
            }
            NV_TYPE_STRING_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let data_end = ptr + datasize;
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    let rest = &buf[ptr..data_end];
                    let rel_end = rest
                        .iter()
                        .position(|b| *b == 0)
                        .ok_or(NvtreeError::Malformed)?;
                    let s = std::str::from_utf8(&rest[..rel_end]).map_err(|_| NvtreeError::InvalidUtf8)?;
                    values.push(s.to_string());
                    ptr += rel_end + 1;
                }
                ptr = data_end;
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::StringArray(values),
                }
            }
            NV_TYPE_NVLIST_ARRAY => {
                if datasize > 0 && ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let data_end = if datasize > 0 { ptr + datasize } else { body_end };
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    if ptr >= data_end {
                        return Err(NvtreeError::Malformed);
                    }
                    let (nested, consumed) = parse_tree(buf, ptr, false)?;
                    values.push(nested);
                    ptr += consumed;

                    // Some producers append explicit END markers between array elements.
                    if ptr + PAIR_HEADER_LEN + 1 <= data_end && buf[ptr] == NV_TYPE_END {
                        ptr += PAIR_HEADER_LEN + 1;
                    }
                }
                if datasize > 0 {
                    // Tolerate a single trailing terminator byte in bounded payloads.
                    if ptr + 1 == data_end && buf[ptr] == 0 {
                        ptr += 1;
                    }
                    if ptr != data_end {
                        return Err(NvtreeError::Malformed);
                    }
                }
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NestedArray(values),
                }
            }
            NV_TYPE_END => break,
            other => return Err(NvtreeError::UnsupportedType(other)),
        };

        tree.head.insert(pair.name.clone(), pair);
    }

    Ok((tree, TREE_HEADER_LEN + body_len))
}

fn read_u8(buf: &[u8], off: &mut usize) -> Result<u8, NvtreeError> {
    if *off + 1 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let v = buf[*off];
    *off += 1;
    Ok(v)
}

fn read_u16(buf: &[u8], off: &mut usize, byte_order: ByteOrder) -> Result<u16, NvtreeError> {
    if *off + 2 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&buf[*off..*off + 2]);
    *off += 2;
    Ok(match byte_order {
        ByteOrder::Little => u16::from_le_bytes(bytes),
        ByteOrder::Big => u16::from_be_bytes(bytes),
    })
}

fn read_u64(buf: &[u8], off: &mut usize, byte_order: ByteOrder) -> Result<u64, NvtreeError> {
    if *off + 8 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[*off..*off + 8]);
    *off += 8;
    Ok(match byte_order {
        ByteOrder::Little => u64::from_le_bytes(bytes),
        ByteOrder::Big => u64::from_be_bytes(bytes),
    })
}

fn write_u16(out: &mut Vec<u8>, value: u16, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn write_u64(out: &mut Vec<u8>, value: u64, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn host_byte_order() -> ByteOrder {
    if cfg!(target_endian = "big") {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    }
}

#[cfg(target_os = "freebsd")]
#[allow(non_camel_case_types)]
type nvlist_t = c_void;

#[cfg(target_os = "freebsd")]
fn freebsd_unpack_with_libnv(buf: &[u8]) -> Result<Nvtree, NvtreeError> {
    let root = unsafe { nvlist_unpack(buf.as_ptr().cast::<c_void>(), buf.len(), 0) };
    if root.is_null() {
        return Err(NvtreeError::Malformed);
    }

    let out = unsafe { nvlist_to_tree(root) };
    unsafe { nvlist_destroy(root) };
    out
}

#[cfg(target_os = "freebsd")]
unsafe fn nvlist_to_tree(nvl: *const nvlist_t) -> Result<Nvtree, NvtreeError> {
    if nvl.is_null() {
        return Err(NvtreeError::Malformed);
    }
    let mut out = nvtree_create(0);
    let mut cookie: *mut c_void = std::ptr::null_mut();

    loop {
        let mut ty: c_int = 0;
        let name_ptr = unsafe { nvlist_next(nvl, &mut ty, &mut cookie) };
        if name_ptr.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr(name_ptr) }
            .to_str()
            .map_err(|_| NvtreeError::InvalidUtf8)?
            .to_string();

        let pair = match ty as u8 {
            NV_TYPE_NULL => nvtree_null(&name),
            NV_TYPE_BOOL => nvtree_bool(&name, unsafe { nvlist_get_bool(nvl, name_ptr) }),
            NV_TYPE_NUMBER => nvtree_number(&name, unsafe { nvlist_get_number(nvl, name_ptr) }),
            NV_TYPE_STRING => {
                let s_ptr = unsafe { nvlist_get_string(nvl, name_ptr) };
                if s_ptr.is_null() {
                    return Err(NvtreeError::Malformed);
                }
                let s = unsafe { CStr::from_ptr(s_ptr) }
                    .to_str()
                    .map_err(|_| NvtreeError::InvalidUtf8)?;
                nvtree_string(&name, s)
            }
            NV_TYPE_NVLIST => {
                let child = unsafe { nvlist_get_nvlist(nvl, name_ptr) };
                let nested = unsafe { nvlist_to_tree(child)? };
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::Nested(Box::new(nested)),
                }
            }
            NV_TYPE_BOOL_ARRAY => {
                let mut nitems = 0usize;
                let data = unsafe { nvlist_get_bool_array(nvl, name_ptr, &mut nitems) };
                if data.is_null() && nitems > 0 {
                    return Err(NvtreeError::Malformed);
                }
                let values = if nitems == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(data, nitems) }.to_vec()
                };
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::BoolArray(values),
                }
            }
            NV_TYPE_NUMBER_ARRAY => {
                let mut nitems = 0usize;
                let data = unsafe { nvlist_get_number_array(nvl, name_ptr, &mut nitems) };
                if data.is_null() && nitems > 0 {
                    return Err(NvtreeError::Malformed);
                }
                let values = if nitems == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(data, nitems) }.to_vec()
                };
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NumberArray(values),
                }
            }
            NV_TYPE_STRING_ARRAY => {
                let mut nitems = 0usize;
                let data = unsafe { nvlist_get_string_array(nvl, name_ptr, &mut nitems) };
                if data.is_null() && nitems > 0 {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                if nitems > 0 {
                    for s_ptr in unsafe { std::slice::from_raw_parts(data, nitems) } {
                        if s_ptr.is_null() {
                            return Err(NvtreeError::Malformed);
                        }
                        let s = unsafe { CStr::from_ptr(*s_ptr) }
                            .to_str()
                            .map_err(|_| NvtreeError::InvalidUtf8)?;
                        values.push(s.to_string());
                    }
                }
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::StringArray(values),
                }
            }
            NV_TYPE_NVLIST_ARRAY => {
                let mut nitems = 0usize;
                let data = unsafe { nvlist_get_nvlist_array(nvl, name_ptr, &mut nitems) };
                if data.is_null() && nitems > 0 {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                if nitems > 0 {
                    for child in unsafe { std::slice::from_raw_parts(data, nitems) } {
                        values.push(unsafe { nvlist_to_tree(*child)? });
                    }
                }
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NestedArray(values),
                }
            }
            other => return Err(NvtreeError::UnsupportedType(other)),
        };
        out.head.insert(pair.name.clone(), pair);
    }

    Ok(out)
}

#[cfg(target_os = "freebsd")]
#[link(name = "nv")]
unsafe extern "C" {
    #[link_name = "FreeBSD_nvlist_unpack"]
    fn nvlist_unpack(buf: *const c_void, size: usize, flags: c_int) -> *mut nvlist_t;
    #[link_name = "FreeBSD_nvlist_destroy"]
    fn nvlist_destroy(nvl: *mut nvlist_t);
    #[link_name = "FreeBSD_nvlist_next"]
    fn nvlist_next(nvl: *const nvlist_t, typep: *mut c_int, cookiep: *mut *mut c_void)
    -> *const c_char;
    #[link_name = "FreeBSD_nvlist_get_bool"]
    fn nvlist_get_bool(nvl: *const nvlist_t, name: *const c_char) -> bool;
    #[link_name = "FreeBSD_nvlist_get_number"]
    fn nvlist_get_number(nvl: *const nvlist_t, name: *const c_char) -> u64;
    #[link_name = "FreeBSD_nvlist_get_string"]
    fn nvlist_get_string(nvl: *const nvlist_t, name: *const c_char) -> *const c_char;
    #[link_name = "FreeBSD_nvlist_get_nvlist"]
    fn nvlist_get_nvlist(nvl: *const nvlist_t, name: *const c_char) -> *const nvlist_t;
    #[link_name = "FreeBSD_nvlist_get_bool_array"]
    fn nvlist_get_bool_array(
        nvl: *const nvlist_t,
        name: *const c_char,
        nitemsp: *mut usize,
    ) -> *const bool;
    #[link_name = "FreeBSD_nvlist_get_number_array"]
    fn nvlist_get_number_array(
        nvl: *const nvlist_t,
        name: *const c_char,
        nitemsp: *mut usize,
    ) -> *const u64;
    #[link_name = "FreeBSD_nvlist_get_string_array"]
    fn nvlist_get_string_array(
        nvl: *const nvlist_t,
        name: *const c_char,
        nitemsp: *mut usize,
    ) -> *const *const c_char;
    #[link_name = "FreeBSD_nvlist_get_nvlist_array"]
    fn nvlist_get_nvlist_array(
        nvl: *const nvlist_t,
        name: *const c_char,
        nitemsp: *mut usize,
    ) -> *const *const nvlist_t;
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn nvtree_create_test() {
        let root = nvtree_create(0);
        assert_eq!(root.flags, 0);
        assert!(root.head.is_empty());
    }

    #[test]
    fn nvtree_find_test() {
        let name = "number";
        let mut root = nvtree_create(0);

        assert!(nvtree_add(&mut root, nvtree_number(name, 5)).is_none());
        assert!(nvtree_find(&root, name).is_some());
        assert!(nvtree_find(&root, "missing").is_none());
    }

    #[test]
    fn pair_kinds_cover_all_types() {
        assert_eq!(nvtree_null("n").kind(), NVTREE_NULL);
        assert_eq!(nvtree_bool("b", true).kind(), NVTREE_BOOL);
        assert_eq!(nvtree_number("u", 7).kind(), NVTREE_NUMBER);
        assert_eq!(nvtree_string("s", "x").kind(), NVTREE_STRING);
        assert_eq!(nvtree_tree("t").kind(), NVTREE_NESTED);
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "ba".to_string(),
                value: Nvtvalue::BoolArray(vec![true, false]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_BOOL
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "na".to_string(),
                value: Nvtvalue::NumberArray(vec![1, 2]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_NUMBER
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "sa".to_string(),
                value: Nvtvalue::StringArray(vec!["a".to_string(), "b".to_string()]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_STRING
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "ta".to_string(),
                value: Nvtvalue::NestedArray(vec![nvtree_create(0)]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_NESTED
        );
    }

    #[test]
    fn nvtree_pack_roundtrip_all_scalar_types() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_null("null"));
        nvtree_add(&mut root, nvtree_bool("bool", true));
        nvtree_add(&mut root, nvtree_number("number", 5));
        nvtree_add(&mut root, nvtree_string("string", "hello"));

        let buf = nvtree_pack(&root);
        assert!(!buf.is_empty());

        let unpacked = nvtree_unpack(&buf).expect("unpack should succeed");
        assert_eq!(
            nvtree_find(&unpacked, "null").expect("null key should exist").value,
            Nvtvalue::Null
        );
        assert_eq!(
            nvtree_find(&unpacked, "bool").expect("bool key should exist").value,
            Nvtvalue::Bool(true)
        );
        assert_eq!(
            nvtree_find(&unpacked, "number")
                .expect("number key should exist")
                .value,
            Nvtvalue::Number(5)
        );
        assert_eq!(
            nvtree_find(&unpacked, "string")
                .expect("string key should exist")
                .value,
            Nvtvalue::String("hello".to_string())
        );
    }

    #[test]
    fn nvtree_pack_roundtrip_array_types() {
        let mut root = nvtree_create(0);
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
                value: Nvtvalue::NumberArray(vec![7, 11]),
            },
        );
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "strings".to_string(),
                value: Nvtvalue::StringArray(vec!["a".to_string(), "bc".to_string()]),
            },
        );
        let mut child = nvtree_create(0);
        nvtree_add(&mut child, nvtree_string("name", "inner"));
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
            nvtree_find(&unpacked, "bools")
                .expect("bool array key should exist")
                .value,
            Nvtvalue::BoolArray(vec![true, false, true])
        );
        assert_eq!(
            nvtree_find(&unpacked, "numbers")
                .expect("number array key should exist")
                .value,
            Nvtvalue::NumberArray(vec![7, 11])
        );
        assert_eq!(
            nvtree_find(&unpacked, "strings")
                .expect("string array key should exist")
                .value,
            Nvtvalue::StringArray(vec!["a".to_string(), "bc".to_string()])
        );
        let trees = nvtree_find(&unpacked, "trees").expect("nested array key should exist");
        match &trees.value {
            Nvtvalue::NestedArray(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(
                    nvtree_find(&values[0], "name")
                        .expect("nested tree item should contain name")
                        .value,
                    Nvtvalue::String("inner".to_string())
                );
            }
            _ => panic!("trees should be nested array"),
        }
    }

    #[test]
    fn nvtree_pack_roundtrip_nested_type() {
        let mut root = nvtree_create(0);
        let mut nested = nvtree_tree("child");
        nvtree_add_tree(&mut nested, nvtree_bool("ok", true)).expect("nested add must work");
        nvtree_add_tree(&mut nested, nvtree_string("name", "inner")).expect("nested add must work");
        nvtree_add(&mut root, nested);

        let packed = nvtree_pack(&root);
        let unpacked = nvtree_unpack(&packed).expect("unpack should succeed");

        let child = nvtree_find(&unpacked, "child").expect("child should exist");
        match &child.value {
            Nvtvalue::Nested(tree) => {
                let ok = nvtree_find(tree, "ok").expect("ok should exist in nested tree");
                assert_eq!(ok.value, Nvtvalue::Bool(true));
                let name = nvtree_find(tree, "name").expect("name should exist in nested tree");
                assert_eq!(name.value, Nvtvalue::String("inner".to_string()));
            }
            _ => panic!("child should be nested"),
        }
    }

    #[test]
    fn nvtree_remove_and_destroy_test() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_number("number", 42));
        assert!(nvtree_remove(&mut root, "number").is_some());
        assert!(nvtree_find(&root, "number").is_none());
        assert_eq!(nvtree_destroy(root), 0);
    }

    #[test]
    fn nvtree_unpack_accepts_endian_flagged_opposite_encoding() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_null("null"));
        nvtree_add(&mut root, nvtree_bool("bool", true));
        nvtree_add(&mut root, nvtree_number("number", 12345));
        nvtree_add(&mut root, nvtree_string("string", "endian"));
        let mut child = nvtree_tree("child");
        nvtree_add_tree(&mut child, nvtree_number("n", 7)).expect("nested add must work");
        nvtree_add(&mut root, child);

        let packed = nvtree_pack(&root);
        let mut flipped = packed.clone();
        let src_is_be = cfg!(target_endian = "big");
        let dst_is_be = !src_is_be;
        rewrite_tree_endian(&mut flipped, 0, src_is_be, dst_is_be, true)
            .expect("endianness rewrite should succeed");

        let unpacked = nvtree_unpack(&flipped).expect("unpack should succeed with endian marker");
        assert_eq!(
            nvtree_find(&unpacked, "number")
                .expect("number key should exist")
                .value,
            Nvtvalue::Number(12345)
        );
        assert_eq!(
            nvtree_find(&unpacked, "string")
                .expect("string key should exist")
                .value,
            Nvtvalue::String("endian".to_string())
        );
        let child = nvtree_find(&unpacked, "child").expect("child should exist");
        match &child.value {
            Nvtvalue::Nested(tree) => {
                assert_eq!(
                    nvtree_find(tree, "n").expect("nested n should exist").value,
                    Nvtvalue::Number(7)
                );
            }
            _ => panic!("child should be nested"),
        }
    }

    #[test]
    fn nvtree_unpack_accepts_nvlist_array_with_zero_datasize() {
        let mut root = nvtree_create(0);
        let mut item = nvtree_create(0);
        nvtree_add(&mut item, nvtree_string("devnode", "dsp0"));
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "dsps".to_string(),
                value: Nvtvalue::NestedArray(vec![item]),
            },
        );

        let mut packed = nvtree_pack(&root);
        let is_be = (packed[2] & NVTREE_FLAG_BIG_ENDIAN) != 0;
        // First/root pair starts immediately after tree header.
        let pair_off = TREE_HEADER_LEN;
        assert_eq!(packed[pair_off], NV_TYPE_NVLIST_ARRAY);
        write_u64_raw(&mut packed, pair_off + 3, 0, is_be);

        let unpacked = nvtree_unpack(&packed).expect("should unpack zero-datasize nvlist array");
        let dsps = nvtree_find(&unpacked, "dsps").expect("dsps key should exist");
        match &dsps.value {
            Nvtvalue::NestedArray(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(
                    nvtree_find(&values[0], "devnode")
                        .expect("devnode key should exist")
                        .value,
                    Nvtvalue::String("dsp0".to_string())
                );
            }
            _ => panic!("dsps should be nvlist array"),
        }
    }
}
