#include <cstdlib>
#include <cstring>
#include <string>

#include "nvtpp.h"

extern "C" {
int cpp_nvtpp_pack_scalars(unsigned char **out_buf, size_t *out_len) {
    nvtpp::Tree tree;
    nvtpp::packed_t packed;
    unsigned char *buf;

    if (out_buf == nullptr || out_len == nullptr) {
        return 0;
    }

    tree.add(new nvtpp::Null("null"));
    tree.add(new nvtpp::Bool("bool", true));
    tree.add(new nvtpp::Number("number", 42));
    tree.add(new nvtpp::String("string", "hello-from-cpp"));

    if (!tree.pack(packed) || packed.empty()) {
        return 0;
    }

    buf = static_cast<unsigned char *>(std::malloc(packed.size()));
    if (buf == nullptr) {
        return 0;
    }

    std::memcpy(buf, packed.data(), packed.size());
    *out_buf = buf;
    *out_len = packed.size();
    return 1;
}

int cpp_nvtpp_pack_nested(unsigned char **out_buf, size_t *out_len) {
    nvtpp::Tree tree;
    nvtpp::packed_t packed;
    unsigned char *buf;
    nvtpp::Tree *child;

    if (out_buf == nullptr || out_len == nullptr) {
        return 0;
    }

    child = new nvtpp::Tree("child");
    child->add(new nvtpp::Bool("ok", true));
    child->add(new nvtpp::String("name", "inner-cpp"));
    tree.add(child);

    if (!tree.pack(packed) || packed.empty()) {
        return 0;
    }

    buf = static_cast<unsigned char *>(std::malloc(packed.size()));
    if (buf == nullptr) {
        return 0;
    }

    std::memcpy(buf, packed.data(), packed.size());
    *out_buf = buf;
    *out_len = packed.size();
    return 1;
}

int cpp_nvtpp_unpack_validate_scalars(const unsigned char *buf, size_t len) {
    nvtpp::Tree tree;
    nvtpp::Pair *pair;
    bool b;
    uint64_t n;
    std::string s;

    if (buf == nullptr || len == 0) {
        return 0;
    }

    try {
        if (!tree.unpack((void *)buf, len)) {
            return 0;
        }
    } catch (...) {
        return 0;
    }

    pair = tree.find("null");
    if (pair == nullptr || pair->type() != nvtpp::Type::NULLPTR) {
        return 0;
    }

    pair = tree.find("bool");
    if (pair == nullptr || pair->type() != nvtpp::Type::BOOL) {
        return 0;
    }
    pair->get(b);
    if (!b) {
        return 0;
    }

    pair = tree.find("number");
    if (pair == nullptr || pair->type() != nvtpp::Type::NUMBER) {
        return 0;
    }
    pair->get(n);
    if (n != 99) {
        return 0;
    }

    pair = tree.find("string");
    if (pair == nullptr || pair->type() != nvtpp::Type::STRING) {
        return 0;
    }
    pair->get(s);
    if (s != "hello-from-rust-to-cpp") {
        return 0;
    }

    return 1;
}

int cpp_nvtpp_unpack_validate_nested(const unsigned char *buf, size_t len) {
    nvtpp::Tree tree;
    nvtpp::Pair *pair;
    nvtpp::Tree *child;
    bool b;
    std::string s;

    if (buf == nullptr || len == 0) {
        return 0;
    }

    try {
        if (!tree.unpack((void *)buf, len)) {
            return 0;
        }
    } catch (...) {
        return 0;
    }

    pair = tree.find("child");
    if (pair == nullptr || pair->type() != nvtpp::Type::TREE) {
        return 0;
    }

    child = dynamic_cast<nvtpp::Tree *>(pair);
    if (child == nullptr) {
        return 0;
    }

    pair = child->find("ok");
    if (pair == nullptr || pair->type() != nvtpp::Type::BOOL) {
        return 0;
    }
    pair->get(b);
    if (!b) {
        return 0;
    }

    pair = child->find("name");
    if (pair == nullptr || pair->type() != nvtpp::Type::STRING) {
        return 0;
    }
    pair->get(s);
    if (s != "inner-rust-to-cpp") {
        return 0;
    }

    return 1;
}
}
