#[test]
fn invalid_rate_factor_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_rate.rs");
}

#[test]
fn mixed_rates_fail_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/mixed_rates.rs");
}

#[test]
fn down_rate_not_supported_v1() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/down_rate.rs");
}

#[test]
fn array_with_invalid_rate_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/array_with_invalid_rate.rs");
}

#[test]
fn array_with_double_rate_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/array_with_double_rate.rs");
}

#[test]
fn cross_rate_kind_mismatch_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/cross_rate_kind_mismatch.rs");
}

#[test]
fn multi_error_type_mismatch_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/multi_error_type_mismatch.rs");
}

#[test]
fn multi_error_mixed_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/multi_error_mixed.rs");
}

#[test]
fn multi_parse_error_top_level_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/multi_parse_error_top_level.rs");
}

#[test]
fn multi_parse_error_in_node_block_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/multi_parse_error_in_node_block.rs");
}

#[test]
fn multi_parse_error_in_connection_block_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/multi_parse_error_in_connection_block.rs");
}
