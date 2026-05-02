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
