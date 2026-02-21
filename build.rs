fn main() {
    println!("cargo:rerun-if-changed=tests/native/nvtree_pack_helper.c");
    println!("cargo:rerun-if-changed=tests/native/nvtpp_pack_helper.cc");
    println!("cargo:rerun-if-changed=/usr/src/sys/contrib/libnvtree/nvtree.c");
    println!("cargo:rerun-if-changed=/usr/src/lib/libnvtpp/nvtpp.cc");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "freebsd" {
        return;
    }

    cc::Build::new()
        .file("tests/native/nvtree_pack_helper.c")
        .file("/usr/src/sys/contrib/libnvtree/nvtree.c")
        .include("/usr/src/sys")
        .warnings(false)
        .compile("nvtree_pack_helper");

    cc::Build::new()
        .cpp(true)
        .file("tests/native/nvtpp_pack_helper.cc")
        .file("/usr/src/lib/libnvtpp/nvtpp.cc")
        .include("/usr/src/lib/libnvtpp")
        .flag_if_supported("-std=c++20")
        .warnings(false)
        .compile("nvtpp_pack_helper");

    println!("cargo:rustc-link-lib=nv");
}
