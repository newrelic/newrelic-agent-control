use std::time::Duration;

use wrapper_with_default::WrapperWithDefault;

#[test]
fn test_duration_wrapper_with_default() {
    const DEFAULT_DURATION: Duration = Duration::from_secs(10);

    #[derive(WrapperWithDefault)]
    #[wrapper_default_value(DEFAULT_DURATION)]
    struct Interval(Duration);

    // Check default
    assert_eq!(Duration::from(Interval::default()), Duration::from_secs(10));
    // Check conversion
    let interval: Interval = Duration::from_secs(1).into();
    let duration: Duration = interval.into();
    assert_eq!(duration, Duration::from_secs(1));
}

#[test]
fn test_usize_wrapper_with_default() {
    const USIZE_DEFAULT: usize = 42;

    #[derive(WrapperWithDefault)]
    #[wrapper_default_value(USIZE_DEFAULT)]
    struct Wrapper(usize);

    // Check expected default
    assert_eq!(usize::from(Wrapper::default()), 42);
    // Check conversion
    let wrapper: Wrapper = 10.into();
    let usize_value: usize = wrapper.into();
    assert_eq!(usize_value, 10);
}
