#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <sys/nvtree.h>

int c_nvtree_pack_scalars(uint8_t **out_buf, size_t *out_len) {
    nvtree_t *root;
    void *packed;
    size_t packed_len;

    if (out_buf == NULL || out_len == NULL) {
        return 0;
    }

    root = nvtree_create();
    if (root == NULL) {
        return 0;
    }

    nvtree_add(root, nvtree_null("null"));
    nvtree_add(root, nvtree_bool("bool", true));
    nvtree_add(root, nvtree_number("number", 42));
    nvtree_add(root, nvtree_string("string", "hello-from-c"));

    packed = nvtree_pack(root, &packed_len);
    nvtree_destroy(root);
    if (packed == NULL || packed_len == 0) {
        return 0;
    }

    *out_buf = (uint8_t *)packed;
    *out_len = packed_len;
    return 1;
}

int c_nvtree_pack_nested(uint8_t **out_buf, size_t *out_len) {
    nvtree_t *root;
    nvtree_t *child_tree;
    nvtpair_t *child_pair;
    void *packed;
    size_t packed_len;

    if (out_buf == NULL || out_len == NULL) {
        return 0;
    }

    root = nvtree_create();
    child_tree = nvtree_create();
    if (root == NULL || child_tree == NULL) {
        return 0;
    }

    nvtree_add(child_tree, nvtree_bool("ok", true));
    nvtree_add(child_tree, nvtree_string("name", "inner-c"));
    child_pair = nvtree_nested("child", child_tree);
    nvtree_add(root, child_pair);

    packed = nvtree_pack(root, &packed_len);
    nvtree_destroy(root);
    if (packed == NULL || packed_len == 0) {
        return 0;
    }

    *out_buf = (uint8_t *)packed;
    *out_len = packed_len;
    return 1;
}

int c_nvtree_unpack_validate_scalars(const uint8_t *buf, size_t len) {
    nvtree_t *root;
    nvtpair_t *pair;
    int ok = 0;

    if (buf == NULL || len == 0) {
        return 0;
    }

    root = nvtree_unpack(buf, len);
    if (root == NULL) {
        return 0;
    }

    pair = nvtree_find(root, "null");
    if (pair == NULL || pair->type != NVTREE_NULL) {
        goto done;
    }
    pair = nvtree_find(root, "bool");
    if (pair == NULL || pair->type != NVTREE_BOOL || !pair->value.b) {
        goto done;
    }
    pair = nvtree_find(root, "number");
    if (pair == NULL || pair->type != NVTREE_NUMBER || pair->value.num != 99) {
        goto done;
    }
    pair = nvtree_find(root, "string");
    if (pair == NULL || pair->type != NVTREE_STRING) {
        goto done;
    }
    if (strcmp(pair->value.string, "hello-from-rust-to-c") != 0) {
        goto done;
    }

    ok = 1;
done:
    nvtree_destroy(root);
    return ok;
}

int c_nvtree_unpack_validate_nested(const uint8_t *buf, size_t len) {
    nvtree_t *root;
    nvtpair_t *pair;
    nvtree_t *child;
    int ok = 0;

    if (buf == NULL || len == 0) {
        return 0;
    }

    root = nvtree_unpack(buf, len);
    if (root == NULL) {
        return 0;
    }

    pair = nvtree_find(root, "child");
    if (pair == NULL || pair->type != NVTREE_NESTED || pair->value.tree == NULL) {
        goto done;
    }

    child = pair->value.tree;
    pair = nvtree_find(child, "ok");
    if (pair == NULL || pair->type != NVTREE_BOOL || !pair->value.b) {
        goto done;
    }
    pair = nvtree_find(child, "name");
    if (pair == NULL || pair->type != NVTREE_STRING) {
        goto done;
    }
    if (strcmp(pair->value.string, "inner-rust-to-c") != 0) {
        goto done;
    }

    ok = 1;
done:
    nvtree_destroy(root);
    return ok;
}
