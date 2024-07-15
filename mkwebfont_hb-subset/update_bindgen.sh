#! /usr/bin/env nix-shell
#! nix-shell -i bash --pure -p bash -p rust-bindgen -p rustfmt

bindgen \
    --allowlist-item hb_.* \
    --bitfield-enum hb_subset_flags_t \
    --bitfield-enum hb_subset_sets_t \
    --bitfield-enum hb_ot_name_id_predefined_t \
    --new-type-alias hb_ot_name_id_t \
    wrapper.h \
    -- -Iharfbuzz/src/ \
    | sed "s/HB_SUBSET_SETS_//g" \
    | sed "s/HB_SUBSET_FLAGS_//g" \
    | sed "s/HB_OT_NAME_ID_//g" \
    | rustfmt > bindings.rs
