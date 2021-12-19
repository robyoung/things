use schemars::JsonSchema;

use serde::{
    de::{self, Unexpected, Visitor},
    Deserialize, Serialize,
};

#[derive(Copy, Clone, PartialEq, Debug, JsonSchema)]
pub struct Id {
    agent: u32,
    id: u32,
}

impl Id {
    pub fn new(agent: u32, id: u32) -> Self {
        Id { agent, id }
    }
}

struct IdVisitor;

impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let id = format!("{}:{}", self.agent, self.id);
        serializer.serialize_str(id.as_str())
    }
}

impl<'de> Visitor<'de> for IdVisitor {
    type Value = Id;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("expecting a colon separated pair of integers")
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
        if let Some(i) = s.find(":") {
            if let (Ok(agent), Ok(id)) = (s[..i].parse::<u32>(), s[i + 1..].parse::<u32>()) {
                return Ok(Id { agent, id });
            }
        }
        Err(de::Error::invalid_value(Unexpected::Str(s), &self))
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(IdVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_test::{assert_tokens, Token};

    #[test]
    fn serde_id() {
        let id = Id { agent: 1, id: 2 };

        assert_tokens(&id, &[Token::Str("1:2")]);
    }
}
