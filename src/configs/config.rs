use std::hash::{Hasher, Hash};

use regex::Regex;

#[derive(Clone, Debug)]
pub struct KV {
    pub key: Key,
    pub value: Value
}

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    String(Box<str>),
    NoCaseString(NoCaseStr)
}
#[derive(Clone, Debug)]
pub enum Value {
    String(Box<str>),
    Regex(Regex)
}

#[derive(Clone, Debug)]
pub struct NoCaseStr {
    value: Box<str>
}

impl PartialEq for NoCaseStr {
    fn eq(&self, other: &Self) -> bool {
        return self.value.to_lowercase() == other.value.to_lowercase()
    }
}

impl Hash for NoCaseStr {
    #[inline] 
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.value.to_lowercase().hash(hasher);
    }
}

impl Eq for NoCaseStr {}

impl NoCaseStr {
    pub fn new(s: &str) -> Self {
        Self{ value: s.into() }
    }
    pub fn inner_value<'t>(&'t self) -> &'t Box<str> {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_case_test() {
        let str1 = NoCaseStr::new("TEST");
        let str2 = NoCaseStr::new("test");
        assert_eq!(str1, str2);
    }
}
