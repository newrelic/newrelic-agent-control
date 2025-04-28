use std::collections::BTreeMap;

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
