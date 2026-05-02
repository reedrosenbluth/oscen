#[test]
fn invalid_rate_factor_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_rate.rs");
}
