use schemars::schema_for;
use things_server::lists::{List, ListItem};

macro_rules! write_schema {
    ($model:ty, $name:expr) => {{
        let schema = schema_for!($model);
        let output = serde_json::to_string_pretty(&schema).unwrap();
        std::fs::write(format!("../schemas/{}.json", $name), output).unwrap();
    }};
}

fn main() {
    write_schema!(ListItem, "item");
    write_schema!(List, "list");
}
