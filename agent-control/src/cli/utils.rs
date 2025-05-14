use std::collections::BTreeMap;

/// Parses a string of key-value pairs separated by commas.
///
/// The equal sign character `=` is used to separate the key from the value,
/// and the comma character `,` to separate the pairs.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
/// use newrelic_agent_control::cli::utils::parse_key_value_pairs;
///
/// let data = "key1=value1, key2=value2, key3=value3";
/// let parsed = parse_key_value_pairs(data);
/// assert_eq!(parsed, BTreeMap::from([
///     ("key1".to_string(), "value1".to_string()),
///     ("key2".to_string(), "value2".to_string()),
///     ("key3".to_string(), "value3".to_string()),
/// ]));
/// ```
pub fn parse_key_value_pairs(data: &str) -> BTreeMap<String, String> {
    let pairs = data.split(',');
    let key_values = pairs.map(|pair| pair.split_once('='));
    let valid_key_values = key_values.flatten();
    let parsed_key_values = valid_key_values
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();

    parsed_key_values
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;

    #[rstest]
    #[case::valid_data("key1=value1,key2=value2,key3=value3", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::valid_data_with_surrounding_whitespaces(" key1=value1  ,     key2=value2,key3=value3     ", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::data_with_invalid_key_value_pair("key1=value1,key2/value2,key3=value3", BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ]))]
    #[case::key_value_pair_with_two_equal_signs("key1=test-value-with=sign", BTreeMap::from([
        ("key1".to_string(), "test-value-with=sign".to_string()),
    ]))]
    #[case::invalid_data("invalid data", BTreeMap::new())]
    #[case::empty("", BTreeMap::new())]
    fn test_parse_key_value_pairs(#[case] data: &str, #[case] expected: BTreeMap<String, String>) {
        assert_eq!(parse_key_value_pairs(data), expected);
    }
}
