use std::str::FromStr;

use regex::Regex;
use serde::{ser::SerializeSeq, Deserialize, Deserializer, Serializer};

pub fn serialize_regex<S>(regex: &Regex, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(regex.as_str())
}

pub fn deserialize_regex<'de, D>(deserializer: D) -> Result<Regex, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Regex::from_str(&s).map_err(serde::de::Error::custom)
}

pub fn serialize_opt_vec_regex<S>(
    regexes: &Option<Vec<Regex>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match regexes {
        Some(regexes) => {
            let mut seq = serializer.serialize_seq(Some(regexes.len()))?;

            for regex in regexes {
                seq.serialize_element(regex.as_str())?;
            }

            seq.end()
        }
        None => serializer.serialize_none(),
    }
}

pub fn deserialize_opt_vec_regex<'de, D>(deserializer: D) -> Result<Option<Vec<Regex>>, D::Error>
where
    D: Deserializer<'de>,
{
    let regexes_str: Option<Vec<String>> = Option::deserialize(deserializer)?;
    let Some(regexes_str) = regexes_str else {
        return Ok(None);
    };

    let mut regexes: Vec<Regex> = Vec::with_capacity(regexes_str.len());

    for regex_str in regexes_str {
        let regex = Regex::from_str(&regex_str).map_err(serde::de::Error::custom)?;
        regexes.push(regex);
    }

    Ok(Some(regexes))
}

#[cfg(test)]
mod tests {
    use regex::Regex;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_serialize_deserialize_regex() {
        #[derive(Serialize, Deserialize)]
        struct A {
            #[serde(
                deserialize_with = "crate::serde_ext::deserialize_regex",
                serialize_with = "crate::serde_ext::serialize_regex"
            )]
            reg_field: Regex,
        }

        let record = A {
            reg_field: Regex::new("abc: (.+)").unwrap(),
        };

        let serialized_record = serde_json::to_string(&record).unwrap();
        assert_eq!(r#"{"reg_field":"abc: (.+)"}"#, &serialized_record);

        let des_record: A = serde_json::from_str(&serialized_record).unwrap();
        assert!(des_record.reg_field.is_match("abc: foo"));

        let captures = des_record.reg_field.captures("abc: foo").unwrap();
        assert_eq!("foo", captures.get(1).unwrap().as_str());
    }

    #[test]
    fn test_serialize_deserialize_opt_vec_regex() {
        #[derive(Serialize, Deserialize)]
        struct A {
            #[serde(
                deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex",
                serialize_with = "crate::serde_ext::serialize_opt_vec_regex"
            )]
            reg_field: Option<Vec<Regex>>,

            #[serde(
                deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex",
                serialize_with = "crate::serde_ext::serialize_opt_vec_regex"
            )]
            reg_field2: Option<Vec<Regex>>,

            #[serde(
                deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex",
                serialize_with = "crate::serde_ext::serialize_opt_vec_regex"
            )]
            reg_field3: Option<Vec<Regex>>,
        }

        let record = A {
            reg_field: None,
            reg_field2: Some(vec![]),
            reg_field3: Some(vec![Regex::new("abc: (.+)").unwrap()]),
        };

        let serialized_record = serde_json::to_string(&record).unwrap();
        assert_eq!(
            r#"{"reg_field":null,"reg_field2":[],"reg_field3":["abc: (.+)"]}"#,
            &serialized_record
        );

        let des_record: A = serde_json::from_str(&serialized_record).unwrap();

        assert!(des_record.reg_field.is_none());

        assert!(des_record.reg_field2.is_some());
        assert!(des_record.reg_field2.unwrap().is_empty());

        assert!(des_record.reg_field3.is_some());
        assert!(des_record.reg_field3.unwrap()[0].is_match("abc: foo"));
    }
}
