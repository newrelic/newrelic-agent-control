use std::collections::BTreeMap;

/// Parses a string of key-value pairs separated by commas.
///
/// The equal sign character `=` is used to separate the key from the value,
/// and the comma character `,` to separate the pairs.
///
/// # Examples
///
/// ```
/// let data = "key1=value1, key2=value2, key3=value3";
/// let parsed = parse_key_value_pairs(data);
/// assert_eq!(parsed, Some(BTreeMap::from([
///     ("key1".to_string(), "value1".to_string()),
///     ("key2".to_string(), "value2".to_string()),
///     ("key3".to_string(), "value3".to_string()),
/// ])));
/// ```
pub fn parse_key_value_pairs(data: &str) -> Option<BTreeMap<String, String>> {
    let mut parsed_key_values = BTreeMap::new();

    let pairs = data.split(',');
    let key_values = pairs.map(|pair| pair.split_once('='));
    let valid_key_values = key_values.flatten();
    valid_key_values.for_each(|(key, value)| {
        parsed_key_values.insert(key.trim().to_string(), value.trim().to_string());
    });

    match parsed_key_values.is_empty() {
        true => None,
        false => Some(parsed_key_values),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;

    #[rstest]
    #[case::valid_data("key1=value1,key2=value2,key3=value3", Some(BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ])))]
    #[case::valid_data_with_surrounding_whitespaces(" key1=value1  ,     key2=value2,key3=value3     ", Some(BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key2".to_string(), "value2".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ])))]
    #[case::data_with_invalid_key_value_pair("key1=value1,key2/value2,key3=value3", Some(BTreeMap::from([
        ("key1".to_string(), "value1".to_string()),
        ("key3".to_string(), "value3".to_string()),
    ])))]
    #[case::key_value_pair_with_two_equal_signs("key1=test-value-with=sign", Some(BTreeMap::from([
        ("key1".to_string(), "test-value-with=sign".to_string()),
    ])))]
    #[case::invalid_data("invalid data", None)]
    #[case::empty("", None)]
    fn test_parse_key_value_pairs(
        #[case] data: &str,
        #[case] expected: Option<BTreeMap<String, String>>,
    ) {
        assert_eq!(parse_key_value_pairs(data), expected);
    }
}
