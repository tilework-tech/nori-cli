#![allow(clippy::unwrap_used)]

use nori_providers::masked_key;
use pretty_assertions::assert_eq;

#[test]
fn known_prefixes_are_shown_and_the_rest_is_masked() {
    let keys = [
        "sk-ant-api03-SECRET",
        "sk-ant-oat01-SECRET",
        "sk-proj-SECRET",
        "sk-svcacct-SECRET",
        "sk-or-v1-SECRET",
        "sk-SECRET",
        "AIzaSECRET",
        "xai-SECRET",
        "gsk_SECRET",
    ];
    assert_eq!(
        keys.map(masked_key),
        [
            "sk-ant-api03-******",
            "sk-ant-oat01-******",
            "sk-proj-******",
            "sk-svcacct-******",
            "sk-or-v1-******",
            "sk-******",
            "AIza******",
            "xai-******",
            "gsk_******",
        ]
        .map(str::to_string)
    );
}

#[test]
fn keys_without_a_known_prefix_are_fully_masked() {
    assert_eq!(
        ["SECRET", "crsr_SECRET", "", "  sk-proj-SECRET"].map(masked_key),
        ["******", "***********", "", "****************"].map(str::to_string)
    );
}

#[test]
fn a_prefix_is_only_shown_once_it_is_complete() {
    let typed: Vec<_> = ["s", "sk", "sk-", "sk-p", "sk-proj", "sk-proj-", "sk-proj-S"]
        .into_iter()
        .map(masked_key)
        .collect();
    assert_eq!(
        typed,
        ["*", "**", "sk-", "sk-*", "sk-****", "sk-proj-", "sk-proj-*"]
    );
}
