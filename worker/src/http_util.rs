use std::collections::HashMap;

use worker::Headers;

pub fn merge_headers(
    original_headers: HashMap<String, String>,
    extra_headers: HashMap<String, String>,
) -> Headers {
    let mut new_headers = Headers::new();
    for (key, value) in original_headers
        .into_iter()
        .chain(extra_headers.into_iter())
    {
        new_headers.set(&key, &value);
    }
    new_headers
}
