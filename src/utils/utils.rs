use std::ops::Deref;

use crate::configs::config::Value;

pub fn value_match(s1: &str, s2: &Value) -> bool {
    match s2 {
        Value::String(string_value) => {
            if *s1 == *string_value.deref() {
                return true;
            } else {
                return false;
            }
        },
        Value::Regex(regex_value) => {
            return regex_value.is_match(s1);
        }
    }
}
